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

    private fun decoded(url: String) = java.net.URLDecoder.decode(url, "UTF-8")

    /** Another device of the same account already made it: find it by the marker, oldest first. */
    @Test fun findsTheMarkedFolderNotByName() {
        val http = Fake(HttpResponse(200, emptyMap(), """{"files":[{"id":"OLDEST"},{"id":"NEWER"}]}"""))
        assertEquals("OLDEST", DriveApi(http).ensureFolder("t"))
        val url = decoded(http.sent[0].second)
        assertTrue(url, url.contains("appProperties has { key='seogi' and value='recordings-root' }"))
        assertTrue(url.contains("trashed = false"))
        assertTrue(url.contains("mimeType = 'application/vnd.google-apps.folder'"))
        assertTrue(url.contains("orderBy=createdTime"))
        assertTrue(!url.contains("name ="))
        assertEquals("Bearer t", http.sent[0].third["Authorization"])
    }

    private class Recording(vararg answers: HttpResponse) : Http {
        val queue = answers.toMutableList()
        var lastBody: String? = null
        override fun send(method: String, url: String, headers: Map<String, String>, body: ByteArray?): HttpResponse {
            if (method == "POST") lastBody = body?.decodeToString()
            return queue.removeAt(0)
        }
    }

    @Test fun makesTheFolderWithTheMarkerWhenThereIsNone() {
        val http = Recording(HttpResponse(200, emptyMap(), """{"files":[]}"""), HttpResponse(200, emptyMap(), """{"id":"F2"}"""))
        assertEquals("F2", DriveApi(http).ensureFolder("t"))
        val body = http.lastBody!!
        assertTrue(body, body.contains("\"appProperties\":{\"seogi\":\"recordings-root\"}"))
        assertTrue(body.contains("서기 폰 녹음"))
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

    @Test fun folderIsUsableOnlyWhenThereAndNotTrashed() {
        val folder = "application/vnd.google-apps.folder"
        val http = Fake(
            HttpResponse(200, emptyMap(), """{"id":"F","trashed":false,"mimeType":"$folder","appProperties":{"seogi":"recordings-root"}}"""),
            HttpResponse(200, emptyMap(), """{"id":"F","trashed":true,"mimeType":"$folder","appProperties":{"seogi":"recordings-root"}}"""),
            HttpResponse(404, emptyMap(), "not found"),
            HttpResponse(200, emptyMap(), """{"id":"F","trashed":false,"mimeType":"$folder"}"""),
        )
        val d = DriveApi(http)
        assertTrue(d.folderUsable("t", "F"))
        assertEquals(false, d.folderUsable("t", "F")) // trashed
        assertEquals(false, d.folderUsable("t", "F")) // gone
        assertEquals(false, d.folderUsable("t", "F")) // same name, no marker
        assertTrue(http.sent[0].second.contains("trashed"))
    }

    /** Review 2: the trash is files.update trashed=true sent as a real PATCH, never a delete of the file. */
    @Test fun trashIsAPatchNotADelete() {
        val http = Fake(HttpResponse(200, emptyMap(), """{"id":"A"}"""), HttpResponse(404, emptyMap(), ""))
        val d = DriveApi(http)
        assertTrue(d.trash("t", "A"))
        assertEquals(false, d.trash("t", "GONE"))
        assertEquals("PATCH", http.sent[0].first)
        assertTrue(http.sent[0].third["X-HTTP-Method-Override"] == null)
        assertTrue(http.sent.none { it.first == "DELETE" })
    }

    @Test fun givingUpASessionIsADeleteOfTheSessionOnly() {
        val http = Fake(HttpResponse(499, emptyMap(), ""))
        DriveApi(http).cancelSession("https://up/s1")
        assertEquals("DELETE", http.sent[0].first)
        assertEquals("https://up/s1", http.sent[0].second)
    }
}
