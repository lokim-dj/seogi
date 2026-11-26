package io.github.lokimdj.seogi.upload

import io.github.lokimdj.seogi.data.Recording
import io.github.lokimdj.seogi.data.RecordingStore
import io.github.lokimdj.seogi.data.Sidecar
import io.github.lokimdj.seogi.data.UploadError
import io.github.lokimdj.seogi.data.UploadState
import java.io.Closeable
import java.io.IOException

/** The phone's copy of a recording, read in pieces. */
interface AudioSource : Closeable {
    val size: Long
    /** Reads up to [length] bytes from [offset] into [buf] at [at]; returns how many were read. */
    fun read(offset: Long, buf: ByteArray, at: Int, length: Int): Int
}

/** An access token, or the user has to sign in again. */
sealed interface TokenResult {
    data class Ok(val token: String) : TokenResult
    data object NeedLogin : TokenResult
}

/**
 * Uploads every recording that still needs it, oldest first, one at a time:
 * the sidecar JSON, then the audio in 8 MB pieces through a resumable
 * session. Progress is saved after every piece, so an upload stopped by the
 * network or by the system continues where it was, not from the start.
 */
class Uploader(
    private val store: RecordingStore,
    private val drive: Drive,
    private val tokens: suspend () -> TokenResult,
    private val open: (Recording) -> AudioSource?,
    private val folder: FolderCache,
    private val device: String,
    private val appVersion: String,
    private val chunkSize: Int = CHUNK,
) {
    /** Remembers the Drive folder between runs. */
    interface FolderCache {
        var folderId: String?
    }

    enum class Outcome { DONE, RETRY, NEED_LOGIN }

    suspend fun runAll(): Outcome {
        var outcome = Outcome.DONE
        while (true) {
            val next = store.all.value.filter { it.needsUpload }.minByOrNull { it.startedAt } ?: break
            when (val o = uploadOne(next)) {
                Outcome.DONE -> continue
                Outcome.NEED_LOGIN -> return o
                Outcome.RETRY -> {
                    outcome = Outcome.RETRY
                    break
                }
            }
        }
        return outcome
    }

    private suspend fun uploadOne(start: Recording): Outcome {
        val id = start.id
        val source = open(start)
        if (source == null) {
            fail(id, UploadError.MISSING_ON_PHONE)
            return Outcome.DONE
        }
        source.use { src ->
            var authRetried = false
            var folderRetried = false
            while (true) {
                val token = when (val t = tokens()) {
                    is TokenResult.Ok -> t.token
                    TokenResult.NeedLogin -> {
                        fail(id, UploadError.NEED_LOGIN)
                        return Outcome.NEED_LOGIN
                    }
                }
                try {
                    store.update(id) { it.copy(upload = UploadState.UPLOADING, error = null) }
                    send(token, store.get(id) ?: return Outcome.DONE, src)
                    return Outcome.DONE
                } catch (e: DriveHttpException) {
                    when {
                        e.code == 401 && !authRetried -> authRetried = true
                        e.code == 401 || (e.code == 403 && e.message.orEmpty().contains("insufficient", true)) -> {
                            fail(id, UploadError.NEED_LOGIN)
                            return Outcome.NEED_LOGIN
                        }
                        e.code == 404 && folder.folderId != null && !folderRetried -> {
                            // The folder was deleted in Drive: make a new one, once.
                            folder.folderId = null
                            store.update(id) { it.copy(sidecarFileId = null, sessionUri = null) }
                            folderRetried = true
                        }
                        else -> return failRetry(id, UploadError.SERVER)
                    }
                } catch (e: IOException) {
                    return failRetry(id, UploadError.OFFLINE)
                }
            }
        }
    }

    private suspend fun send(token: String, r: Recording, src: AudioSource) {
        val folderId = folder.folderId ?: drive.ensureFolder(token).also { folder.folderId = it }
        if (r.sidecarFileId == null) {
            val bytes = Sidecar.json(r, device, appVersion).toByteArray()
            val fileId = drive.uploadSmall(token, folderId, Sidecar.fileName(r.fileName), "application/json", bytes)
            store.update(r.id) { it.copy(sidecarFileId = fileId) }
        }
        val size = src.size
        var session = r.sessionUri
        var offset = 0L
        if (session != null) {
            when (val s = drive.status(token, session, size)) {
                is UploadStatus.Complete -> return done(r.id, s.fileId)
                is UploadStatus.Incomplete -> offset = s.next
                UploadStatus.Expired -> session = null
            }
        }
        if (session == null) {
            session = drive.startSession(token, folderId, r.fileName, r.mimeType, size)
            val s = session
            store.update(r.id) { it.copy(sessionUri = s, progress = 0f) }
        }
        val buf = ByteArray(chunkSize)
        while (true) {
            val want = minOf(chunkSize.toLong(), size - offset).toInt()
            var got = 0
            while (got < want) {
                val n = src.read(offset + got, buf, got, want - got)
                if (n <= 0) throw IOException("short read")
                got += n
            }
            val piece = if (want == chunkSize) buf else buf.copyOf(want)
            when (val s = drive.putChunk(token, session, piece, offset, size)) {
                is UploadStatus.Complete -> return done(r.id, s.fileId)
                is UploadStatus.Incomplete -> {
                    offset = s.next
                    val p = if (size == 0L) 1f else offset.toFloat() / size
                    store.update(r.id) { it.copy(progress = p) }
                }
                UploadStatus.Expired -> {
                    store.update(r.id) { it.copy(sessionUri = null) }
                    throw DriveHttpException(410, "upload session expired")
                }
            }
        }
    }

    private suspend fun done(id: String, fileId: String) =
        store.update(id) { it.copy(upload = UploadState.UPLOADED, progress = 1f, error = null, driveFileId = fileId, sessionUri = null) }

    private suspend fun fail(id: String, error: UploadError) =
        store.update(id) { it.copy(upload = UploadState.FAILED, error = error) }

    private suspend fun failRetry(id: String, error: UploadError): Outcome {
        fail(id, error)
        return Outcome.RETRY
    }

    companion object {
        /** A multiple of 256 KiB, as Drive asks for every piece but the last. */
        const val CHUNK = 8 * 1024 * 1024
    }
}
