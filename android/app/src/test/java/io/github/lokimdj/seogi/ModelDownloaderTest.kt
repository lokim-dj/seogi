package io.github.lokimdj.seogi

import io.github.lokimdj.seogi.dictation.Fetched
import io.github.lokimdj.seogi.dictation.ModelDownloader
import io.github.lokimdj.seogi.dictation.RangeFetch
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import java.io.ByteArrayInputStream
import java.io.IOException
import java.io.InputStream
import java.security.MessageDigest

class ModelDownloaderTest {
    @get:Rule val tmp = TemporaryFolder()

    private val content = ByteArray(700_000) { (it * 31 % 251).toByte() }
    private val sha = MessageDigest.getInstance("SHA-256").digest(content).joinToString("") { "%02x".format(it) }

    /** A server that honours ranges; it can drop the line after [dropAfter] bytes of one answer. */
    private class Server(val content: ByteArray, var honourRange: Boolean = true) : RangeFetch {
        val asked = mutableListOf<Long>()
        var dropAfter: Int? = null
        override fun get(url: String, from: Long): Fetched {
            asked += from
            val start = if (honourRange) from.toInt() else 0
            val bytes = content.copyOfRange(start, content.size)
            val limit = dropAfter
            dropAfter = null
            val body: InputStream = if (limit == null) ByteArrayInputStream(bytes) else object : InputStream() {
                var i = 0
                override fun read(): Int {
                    if (i >= limit) throw IOException("connection reset")
                    return if (i < bytes.size) bytes[i++].toInt() and 0xff else -1
                }
            }
            return Fetched(if (from > 0 && honourRange) 206 else 200, body)
        }
    }

    private fun downloader(server: RangeFetch, sha256: String = sha) =
        ModelDownloader(server, "https://example.invalid/m.gguf", content.size.toLong(), sha256)

    @Test fun downloadsAndNamesTheFileOnlyWhenRight() {
        val part = tmp.root.resolve("m.gguf.part")
        val dest = tmp.root.resolve("m.gguf")
        var last = 0L
        val r = downloader(Server(content)).download(part, dest, { last = it }, { false })
        assertEquals(ModelDownloader.Result.Done, r)
        assertArrayEquals(content, dest.readBytes())
        assertFalse(part.exists())
        assertEquals(content.size.toLong(), last)
    }

    @Test fun anInterruptedDownloadContinuesWhereItStopped() {
        val part = tmp.root.resolve("m.gguf.part")
        val dest = tmp.root.resolve("m.gguf")
        val server = Server(content).apply { dropAfter = 300_000 }
        val d = downloader(server)
        assertTrue(runCatching { d.download(part, dest, {}, { false }) }.exceptionOrNull() is IOException)
        val kept = part.length()
        assertTrue("kept $kept", kept in 1..300_000)
        assertEquals(ModelDownloader.Result.Done, d.download(part, dest, {}, { false }))
        assertEquals(listOf(0L, kept), server.asked)
        assertArrayEquals(content, dest.readBytes())
    }

    @Test fun aServerIgnoringTheRangeStartsOver() {
        val part = tmp.root.resolve("m.gguf.part")
        part.writeBytes(content.copyOfRange(0, 1000))
        val dest = tmp.root.resolve("m.gguf")
        assertEquals(ModelDownloader.Result.Done, downloader(Server(content, honourRange = false)).download(part, dest, {}, { false }))
        assertArrayEquals(content, dest.readBytes())
    }

    @Test fun aWrongFileIsDeletedNotKept() {
        val part = tmp.root.resolve("m.gguf.part")
        val dest = tmp.root.resolve("m.gguf")
        val r = downloader(Server(content), sha256 = "00".repeat(32)).download(part, dest, {}, { false })
        assertTrue(r is ModelDownloader.Result.Wrong)
        assertFalse(part.exists())
        assertFalse(dest.exists())
    }

    @Test fun stoppingKeepsWhatCame() {
        val part = tmp.root.resolve("m.gguf.part")
        val dest = tmp.root.resolve("m.gguf")
        var reads = 0
        val r = downloader(Server(content)).download(part, dest, {}, { reads++ > 1 })
        assertEquals(ModelDownloader.Result.Stopped, r)
        assertTrue(part.length() > 0)
        assertFalse(dest.exists())
    }

    @Test fun aServerErrorIsANetworkProblemToRetry() {
        val part = tmp.root.resolve("m.gguf.part")
        val failing = RangeFetch { _, _ -> Fetched(503, ByteArrayInputStream(ByteArray(0))) }
        assertTrue(runCatching { downloader(failing).download(part, tmp.root.resolve("m.gguf"), {}, { false }) }.exceptionOrNull() is IOException)
    }
}
