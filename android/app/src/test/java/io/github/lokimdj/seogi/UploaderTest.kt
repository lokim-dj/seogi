package io.github.lokimdj.seogi

import io.github.lokimdj.seogi.data.Recording
import io.github.lokimdj.seogi.data.RecordingStore
import io.github.lokimdj.seogi.data.UploadError
import io.github.lokimdj.seogi.data.UploadState
import io.github.lokimdj.seogi.upload.AudioSource
import io.github.lokimdj.seogi.upload.Drive
import io.github.lokimdj.seogi.upload.DriveHttpException
import io.github.lokimdj.seogi.upload.TokenResult
import io.github.lokimdj.seogi.upload.UploadStatus
import io.github.lokimdj.seogi.upload.Uploader
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import java.io.ByteArrayOutputStream
import java.io.File
import java.io.IOException

class UploaderTest {
    @get:Rule val tmp = TemporaryFolder()

    /** A Drive in memory: keeps what each session received. */
    private class FakeDrive : Drive {
        val received = mutableMapOf<String, ByteArrayOutputStream>()
        val small = mutableListOf<String>()
        var sessions = 0
        var failAfterChunks = Int.MAX_VALUE
        var chunkError: Exception = IOException("network")
        var unauthorizedCalls = 0
        var folderCalls = 0

        override fun ensureFolder(token: String): String { folderCalls++; return "F" }
        override fun uploadSmall(token: String, folderId: String, name: String, mime: String, bytes: ByteArray): String {
            if (unauthorizedCalls > 0) { unauthorizedCalls--; throw DriveHttpException(401, "") }
            small += name
            return "J${small.size}"
        }
        override fun startSession(token: String, folderId: String, name: String, mime: String, size: Long): String {
            sessions++
            return "s$sessions".also { received[it] = ByteArrayOutputStream() }
        }
        override fun status(token: String, session: String, size: Long): UploadStatus =
            received[session]?.let { UploadStatus.Incomplete(it.size().toLong()) } ?: UploadStatus.Expired
        override fun putChunk(token: String, session: String, bytes: ByteArray, from: Long, size: Long): UploadStatus {
            if (failAfterChunks-- <= 0) throw chunkError
            val out = received.getValue(session)
            check(from == out.size().toLong()) { "chunk at $from but drive has ${out.size()}" }
            out.write(bytes)
            return if (out.size().toLong() == size) UploadStatus.Complete("A-$session") else UploadStatus.Incomplete(out.size().toLong())
        }
        override fun email(token: String) = null
    }

    private class Bytes(val data: ByteArray) : AudioSource {
        override val size = data.size.toLong()
        override fun read(offset: Long, buf: ByteArray, at: Int, length: Int): Int {
            val n = minOf(length, (data.size - offset).toInt(), 300) // short reads on purpose
            System.arraycopy(data, offset.toInt(), buf, at, n)
            return n
        }
        override fun close() {}
    }

    private val audio = ByteArray(2500) { (it % 251).toByte() }

    private fun uploader(store: RecordingStore, drive: Drive, token: TokenResult = TokenResult.Ok("t"), missing: Set<String> = emptySet()) =
        Uploader(
            store = store, drive = drive, tokens = { token },
            open = { r: Recording -> if (r.id in missing) null else Bytes(audio) },
            folder = object : Uploader.FolderCache { override var folderId: String? = null },
            device = "Phone X", appVersion = "0.1.0", chunkSize = 1024,
        )

    private fun store() = RecordingStore(File(tmp.root, "r.json"))

    @Test fun uploadsSidecarThenAudioInPieces() = runTest {
        val s = store().apply { upsert(recording("a")) }
        val d = FakeDrive()
        assertEquals(Uploader.Outcome.DONE, uploader(s, d).runAll())
        assertEquals(listOf("a.json"), d.small)
        assertArrayEquals(audio, d.received.getValue("s1").toByteArray())
        val r = s.get("a")!!
        assertEquals(UploadState.UPLOADED, r.upload)
        assertEquals("A-s1", r.driveFileId)
        assertEquals(1f, r.progress)
    }

    /** Review focus 3: a cut-off upload continues where it stopped. */
    @Test fun continuesWhereItStopped() = runTest {
        val s = store().apply { upsert(recording("a")) }
        val d = FakeDrive().apply { failAfterChunks = 1 }
        assertEquals(Uploader.Outcome.RETRY, uploader(s, d).runAll())
        assertEquals(UploadError.OFFLINE, s.get("a")!!.error)
        assertEquals(1024, d.received.getValue("s1").size())
        d.failAfterChunks = Int.MAX_VALUE
        assertEquals(Uploader.Outcome.DONE, uploader(s, d).runAll())
        assertEquals(1, d.sessions)
        assertEquals(listOf("a.json"), d.small)
        assertArrayEquals(audio, d.received.getValue("s1").toByteArray())
    }

    /** Review focus 4: a lost login stops loudly, not silently. */
    @Test fun lostLoginIsShown() = runTest {
        val s = store().apply { upsert(recording("a")) }
        assertEquals(Uploader.Outcome.NEED_LOGIN, uploader(s, FakeDrive(), TokenResult.NeedLogin).runAll())
        assertEquals(UploadState.FAILED, s.get("a")!!.upload)
        assertEquals(UploadError.NEED_LOGIN, s.get("a")!!.error)
    }

    @Test fun oneExpiredTokenIsRetriedTwoAreALostLogin() = runTest {
        val s = store().apply { upsert(recording("a")) }
        val d = FakeDrive().apply { unauthorizedCalls = 1 }
        assertEquals(Uploader.Outcome.DONE, uploader(s, d).runAll())
        val s2 = RecordingStore(File(tmp.root, "r2.json")).apply { upsert(recording("b")) }
        val d2 = FakeDrive().apply { unauthorizedCalls = 2 }
        assertEquals(Uploader.Outcome.NEED_LOGIN, uploader(s2, d2).runAll())
        assertEquals(UploadError.NEED_LOGIN, s2.get("b")!!.error)
    }

    /** Review focus 5: a file deleted on the phone is marked, the next one still goes. */
    @Test fun missingFileIsMarkedAndTheRestGoOn() = runTest {
        val s = store().apply {
            upsert(recording("old", startedAt = 1))
            upsert(recording("new", startedAt = 2))
        }
        assertEquals(Uploader.Outcome.DONE, uploader(s, FakeDrive(), missing = setOf("old")).runAll())
        assertEquals(UploadError.MISSING_ON_PHONE, s.get("old")!!.error)
        assertEquals(UploadState.UPLOADED, s.get("new")!!.upload)
    }

    @Test fun serverErrorWaitsAndRetries() = runTest {
        val s = store().apply { upsert(recording("a")) }
        val d = FakeDrive().apply { failAfterChunks = 0; chunkError = DriveHttpException(503, "busy") }
        assertEquals(Uploader.Outcome.RETRY, uploader(s, d).runAll())
        assertEquals(UploadError.SERVER, s.get("a")!!.error)
    }

    @Test fun oldestFirst() = runTest {
        val s = store().apply {
            upsert(recording("b", startedAt = 2))
            upsert(recording("a", startedAt = 1))
        }
        val d = FakeDrive()
        uploader(s, d).runAll()
        assertEquals(listOf("a.json", "b.json"), d.small)
    }
}
