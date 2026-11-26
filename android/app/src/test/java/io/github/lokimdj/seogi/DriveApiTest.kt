package io.github.lokimdj.seogi

import io.github.lokimdj.seogi.upload.DriveApi
import io.github.lokimdj.seogi.upload.DriveFile
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

    /** Looks only in the folder, by exact names, with quotes kept inside the search. */
    @Test fun findsFilesByNameInTheFolder() {
        val http = Fake(
            HttpResponse(
                200, emptyMap(),
                """{"files":[{"id":"J1","name":"it's.json","appProperties":{"seogi_rec":"r1"},"createdTime":"2026-11-26T09:42:55.000Z"},{"id":"A1","name":"a.m4a","sha256Checksum":"ab"}]}""",
            ),
        )
        val found = DriveApi(http).findByName("t", "F", listOf("it's.json", "a.m4a"))
        assertEquals(
            listOf(DriveFile("J1", "it's.json", null, "r1", 1_795_686_175_000), DriveFile("A1", "a.m4a", "ab", null)),
            found,
        )
        val url = decoded(http.sent[0].second)
        assertTrue(url, url.contains("'F' in parents and trashed = false and (name = 'it\\'s.json' or name = 'a.m4a')"))
        assertTrue(url.contains("orderBy=createdTime"))
    }

    @Test fun updatingASmallFileReplacesItsContentOnly() {
        val http = Fake(HttpResponse(200, emptyMap(), """{"id":"J1"}"""))
        DriveApi(http).updateSmall("t", "J1", "application/json", "{}".toByteArray())
        assertEquals("PATCH", http.sent[0].first)
        assertTrue(http.sent[0].second, http.sent[0].second.startsWith("https://www.googleapis.com/upload/drive/v3/files/J1?uploadType=media"))
    }

    @Test fun uploadsCarryTheirRecording() {
        val mark = "\"appProperties\":{\"seogi_rec\":\"rec-1\"}"
        val http = Recording(HttpResponse(200, mapOf("location" to "https://up/s1"), ""), HttpResponse(200, emptyMap(), """{"id":"J1"}"""))
        DriveApi(http).startSession("t", "F", "a.m4a", "audio/mp4", 100, "rec-1")
        assertTrue(http.lastBody!!, http.lastBody!!.contains(mark))
        DriveApi(http).uploadSmall("t", "F", "a.json", "application/json", "{}".toByteArray(), "rec-1")
        assertTrue(http.lastBody!!, http.lastBody!!.contains(mark))
    }
}
