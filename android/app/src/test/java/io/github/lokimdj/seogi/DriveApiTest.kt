package io.github.lokimdj.seogi

import io.github.lokimdj.seogi.upload.DriveApi
import io.github.lokimdj.seogi.upload.Http
import io.github.lokimdj.seogi.upload.HttpResponse
import io.github.lokimdj.seogi.upload.UploadStatus
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class DriveApiTest {
    private class Fake(vararg answers: HttpResponse) : Http {
        val queue = answers.toMutableList()
        val sent = mutableListOf<Triple<String, String, Map<String, String>>>()
        override fun send(method: String, url: String, headers: Map<String, String>, body: ByteArray?): HttpResponse {
            sent += Triple(method, url, headers)
            return queue.removeAt(0)
        }
    }

    @Test fun findsTheAppFolder() {
        val http = Fake(HttpResponse(200, emptyMap(), """{"files":[{"id":"F1"}]}"""))
        assertEquals("F1", DriveApi(http).ensureFolder("t"))
        assertTrue(http.sent[0].second.contains("trashed"))
        assertEquals("Bearer t", http.sent[0].third["Authorization"])
    }

    @Test fun makesTheFolderWhenThereIsNone() {
        val http = Fake(HttpResponse(200, emptyMap(), """{"files":[]}"""), HttpResponse(200, emptyMap(), """{"id":"F2"}"""))
        assertEquals("F2", DriveApi(http).ensureFolder("t"))
        assertEquals("POST", http.sent[1].first)
    }

    @Test fun sessionAddressComesFromLocation() {
        val http = Fake(HttpResponse(200, mapOf("location" to "https://up/s1"), ""))
        assertEquals("https://up/s1", DriveApi(http).startSession("t", "F", "a.m4a", "audio/mp4", 100))
        assertEquals("100", http.sent[0].third["X-Upload-Content-Length"])
    }

    /** Review focus 3: the status query says where to continue. */
    @Test fun statusReadsTheRange() {
        val http = Fake(
            HttpResponse(308, mapOf("range" to "bytes=0-8388607"), ""),
            HttpResponse(308, emptyMap(), ""),
            HttpResponse(201, emptyMap(), """{"id":"A"}"""),
            HttpResponse(404, emptyMap(), ""),
        )
        val d = DriveApi(http)
        assertEquals(UploadStatus.Incomplete(8_388_608), d.status("t", "s", 20_000_000))
        assertEquals("bytes */20000000", http.sent[0].third["Content-Range"])
        assertEquals(UploadStatus.Incomplete(0), d.status("t", "s", 20_000_000))
        assertEquals(UploadStatus.Complete("A"), d.status("t", "s", 20_000_000))
        assertEquals(UploadStatus.Expired, d.status("t", "s", 20_000_000))
    }

    @Test fun chunkSaysWhichBytes() {
        val http = Fake(HttpResponse(308, mapOf("range" to "bytes=0-1023"), ""))
        assertEquals(UploadStatus.Incomplete(1024), DriveApi(http).putChunk("t", "s", ByteArray(1024), 0, 4096))
        assertEquals("bytes 0-1023/4096", http.sent[0].third["Content-Range"])
    }
}
