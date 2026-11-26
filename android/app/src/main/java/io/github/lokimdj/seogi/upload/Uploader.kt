package io.github.lokimdj.seogi.upload

import io.github.lokimdj.seogi.data.Recording
import io.github.lokimdj.seogi.data.RecordingStore
import io.github.lokimdj.seogi.data.Sidecar
import io.github.lokimdj.seogi.data.UploadError
import io.github.lokimdj.seogi.data.UploadState
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.isActive
import kotlinx.coroutines.withContext
import kotlinx.coroutines.runInterruptible
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

    /**
     * One pass over everything waiting, oldest first. A recording that fails
     * for a reason the queue handles itself does not hold up the ones after
     * it; the pass then ends with [Outcome.RETRY] so the queue comes back.
     */
    suspend fun runAll(): Outcome {
        var outcome = Outcome.DONE
        val tried = mutableSetOf<String>()
        while (true) {
            currentCoroutineContext().ensureActive()
            val next = store.all.value
                .filter { it.needsUpload && it.id !in tried }
                .minByOrNull { it.startedAt } ?: break
            tried += next.id
            when (uploadOne(next)) {
                Outcome.DONE -> Unit
                Outcome.NEED_LOGIN -> return Outcome.NEED_LOGIN
                Outcome.RETRY -> outcome = Outcome.RETRY
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
                    if (!currentCoroutineContext().isActive) {
                        // Interrupted by a cancel, not "offline".
                        backToWaiting(id)
                        throw CancellationException("upload stopped")
                    }
                    return failRetry(id, UploadError.OFFLINE)
                } catch (e: CancellationException) {
                    // Stopped by the system or a changed network rule: the
                    // session stays, the next run continues from Drive's count.
                    backToWaiting(id)
                    throw e
                } catch (e: Exception) {
                    // An answer we did not expect: never leave it "uploading".
                    return failRetry(id, UploadError.SERVER)
                }
            }
        }
    }

    private suspend fun send(token: String, r: Recording, src: AudioSource) {
        val folderId = folder.folderId ?: io { drive.ensureFolder(token) }.also { folder.folderId = it }
        if (r.sidecarFileId == null) {
            val bytes = Sidecar.json(r, device, appVersion).toByteArray()
            val fileId = io { drive.uploadSmall(token, folderId, Sidecar.fileName(r.fileName), "application/json", bytes) }
            store.update(r.id) { it.copy(sidecarFileId = fileId) }
        }
        val size = src.size
        var session = r.sessionUri
        var offset = 0L
        if (session != null) {
            val open = session
            when (val s = io { drive.status(token, open, size) }) {
                is UploadStatus.Complete -> return done(r.id, s.fileId)
                is UploadStatus.Incomplete -> offset = s.next
                UploadStatus.Expired -> session = null
            }
        }
        if (session == null) {
            val made = io { drive.startSession(token, folderId, r.fileName, r.mimeType, size) }
            session = made
            store.update(r.id) { it.copy(sessionUri = made, progress = 0f) }
        }
        val current: String = session
        val buf = ByteArray(chunkSize)
        while (true) {
            currentCoroutineContext().ensureActive()
            val want = minOf(chunkSize.toLong(), size - offset).toInt()
            var got = 0
            while (got < want) {
                val n = src.read(offset + got, buf, got, want - got)
                if (n <= 0) throw IOException("short read")
                got += n
            }
            val piece = if (want == chunkSize) buf else buf.copyOf(want)
            when (val s = io { drive.putChunk(token, current, piece, offset, size) }) {
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

    private suspend fun backToWaiting(id: String) = withContext(NonCancellable) {
        store.update(id) { if (it.upload == UploadState.UPLOADING) it.copy(upload = UploadState.WAITING) else it }
    }

    /** Network calls block; this lets a cancelled run stop at the next piece. */
    private suspend fun <T> io(block: () -> T): T = runInterruptible(Dispatchers.IO) { block() }

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
