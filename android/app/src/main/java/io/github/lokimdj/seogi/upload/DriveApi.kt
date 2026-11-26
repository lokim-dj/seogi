package io.github.lokimdj.seogi.upload

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put
import java.net.URLEncoder

/** A status Drive answered with that is not what the call expects. */
class DriveHttpException(val code: Int, message: String) : Exception("Drive $code: $message")

/** Where a resumable upload stands. */
sealed interface UploadStatus {
    /** Drive has bytes `0 until next`. */
    data class Incomplete(val next: Long) : UploadStatus
    data class Complete(val fileId: String) : UploadStatus
    /** The upload session is gone (about a week old); start a new one. */
    data object Expired : UploadStatus
}

/** The Drive calls the uploader makes, with the `drive.file` scope only. */
interface Drive {
    /** The app-made `서기 폰 녹음` folder; made when there is none. */
    fun ensureFolder(token: String): String
    /** The saved folder still exists, is a marked folder, and is not in the trash. */
    fun folderUsable(token: String, folderId: String): Boolean
    fun uploadSmall(token: String, folderId: String, name: String, mime: String, bytes: ByteArray): String
    /** Opens a resumable upload; returns the session address. */
    fun startSession(token: String, folderId: String, name: String, mime: String, size: Long): String
    fun status(token: String, session: String, size: Long): UploadStatus
    fun putChunk(token: String, session: String, bytes: ByteArray, from: Long, size: Long): UploadStatus
    fun email(token: String): String?
    /**
     * Moves a file the app made to the Drive trash (files.update trashed=true,
     * never a permanent delete). False when Drive does not know the file (404).
     */
    fun trash(token: String, fileId: String): Boolean
    /** Gives up a resumable upload that was not finished, so no half file stays behind. */
    fun cancelSession(session: String)
}

/** [Drive] over the Drive REST API v3. */
class DriveApi(private val http: Http = OkHttpHttp) : Drive {
    private val json = Json { ignoreUnknownKeys = true }

    /**
     * Finds the recordings folder by its marker, not by its name, so the
     * phone, the Mac and Windows signed in to one account share one folder:
     * `appProperties seogi=recordings-root`, a folder, not trashed; the oldest
     * when there are several. A folder that only has the same name is never
     * used. Makes one, with the marker, only when there is none.
     */
    override fun ensureFolder(token: String): String {
        val q = "appProperties has { key='$MARK_KEY' and value='$MARK_VALUE' } and " +
            "mimeType = '$FOLDER_MIME' and trashed = false"
        val found = call("GET", "$API/files?spaces=drive&orderBy=createdTime&fields=files(id,createdTime)&q=${enc(q)}", token)
        val files = json.parseToJsonElement(found.body).jsonObject["files"]?.jsonArray ?: JsonArray(emptyList())
        files.firstOrNull()?.let { return it.jsonObject["id"]!!.jsonPrimitive.content }
        val body = buildJsonObject {
            put("name", FOLDER_NAME)
            put("mimeType", FOLDER_MIME)
            put("appProperties", buildJsonObject { put(MARK_KEY, MARK_VALUE) })
        }.toString().toByteArray()
        val made = call("POST", "$API/files?fields=id", token, mapOf("Content-Type" to JSON_UTF8), body)
        return idOf(made.body)
    }

    override fun folderUsable(token: String, folderId: String): Boolean {
        val r = http.send("GET", "$API/files/${enc(folderId)}?fields=id,trashed,mimeType,appProperties", auth(token), null)
        if (r.code == 404) return false
        if (r.code !in 200..299) throw DriveHttpException(r.code, r.body.take(200))
        val o = json.parseToJsonElement(r.body).jsonObject
        val trashed = o["trashed"]?.jsonPrimitive?.content == "true"
        val isFolder = o["mimeType"]?.jsonPrimitive?.content == FOLDER_MIME
        val marked = (o["appProperties"] as? JsonObject)?.get(MARK_KEY)?.jsonPrimitive?.content == MARK_VALUE
        return isFolder && !trashed && marked
    }

    override fun uploadSmall(token: String, folderId: String, name: String, mime: String, bytes: ByteArray): String {
        val boundary = "seogi-" + System.nanoTime()
        val meta = metadata(name, folderId)
        val head = "--$boundary\r\nContent-Type: $JSON_UTF8\r\n\r\n$meta\r\n--$boundary\r\nContent-Type: $mime\r\n\r\n"
        val body = head.toByteArray() + bytes + "\r\n--$boundary--\r\n".toByteArray()
        val r = call(
            "POST", "$UPLOAD/files?uploadType=multipart&fields=id", token,
            mapOf("Content-Type" to "multipart/related; boundary=$boundary"), body,
        )
        return idOf(r.body)
    }

    override fun startSession(token: String, folderId: String, name: String, mime: String, size: Long): String {
        val r = call(
            "POST", "$UPLOAD/files?uploadType=resumable&fields=id", token,
            mapOf(
                "Content-Type" to JSON_UTF8,
                "X-Upload-Content-Type" to mime,
                "X-Upload-Content-Length" to size.toString(),
            ),
            metadata(name, folderId).toByteArray(),
        )
        return r.headers["location"] ?: throw DriveHttpException(r.code, "no upload address")
    }

    override fun status(token: String, session: String, size: Long): UploadStatus =
        answer(http.send("PUT", session, auth(token) + ("Content-Range" to "bytes */$size"), ByteArray(0)))

    override fun putChunk(token: String, session: String, bytes: ByteArray, from: Long, size: Long): UploadStatus {
        val last = from + bytes.size - 1
        return answer(http.send("PUT", session, auth(token) + ("Content-Range" to "bytes $from-$last/$size"), bytes))
    }

    override fun trash(token: String, fileId: String): Boolean {
        val r = http.send(
            "PATCH", "$API/files/${enc(fileId)}?fields=id,trashed",
            auth(token) + ("Content-Type" to JSON_UTF8),
            """{"trashed":true}""".toByteArray(),
        )
        if (r.code == 404) return false
        if (r.code !in 200..299) throw DriveHttpException(r.code, r.body.take(200))
        return true
    }

    override fun cancelSession(session: String) {
        // Drive answers 499 to a cancelled session; any answer is fine here.
        http.send("DELETE", session, emptyMap(), null)
    }

    override fun email(token: String): String? {
        val r = call("GET", "$API/about?fields=user(emailAddress)", token)
        return (json.parseToJsonElement(r.body).jsonObject["user"] as? JsonObject)
            ?.get("emailAddress")?.jsonPrimitive?.content
    }

    private fun answer(r: HttpResponse): UploadStatus = when (r.code) {
        200, 201 -> UploadStatus.Complete(idOf(r.body))
        308 -> UploadStatus.Incomplete(nextFromRange(r.headers["range"]))
        404, 410 -> UploadStatus.Expired
        else -> throw DriveHttpException(r.code, r.body.take(200))
    }

    private fun call(method: String, url: String, token: String, extra: Map<String, String> = emptyMap(), body: ByteArray? = null): HttpResponse {
        val r = http.send(method, url, auth(token) + extra, body)
        if (r.code !in 200..299) throw DriveHttpException(r.code, r.body.take(200))
        return r
    }

    private fun auth(token: String) = mapOf("Authorization" to "Bearer $token")

    private fun metadata(name: String, folderId: String) = buildJsonObject {
        put("name", name)
        put("parents", JsonArray(listOf(JsonPrimitive(folderId))))
    }.toString()

    private fun idOf(body: String) = json.parseToJsonElement(body).jsonObject["id"]!!.jsonPrimitive.content

    companion object {
        const val FOLDER_NAME = "서기 폰 녹음"
        const val FOLDER_MIME = "application/vnd.google-apps.folder"
        /** The marker every Seogi app looks for (Android, and the Mac and Windows app). */
        const val MARK_KEY = "seogi"
        const val MARK_VALUE = "recordings-root"
        const val SCOPE = "https://www.googleapis.com/auth/drive.file"
        private const val API = "https://www.googleapis.com/drive/v3"
        private const val UPLOAD = "https://www.googleapis.com/upload/drive/v3"
        private const val JSON_UTF8 = "application/json; charset=UTF-8"

        private fun enc(s: String) = URLEncoder.encode(s, "UTF-8").replace("+", "%20")

        /** `bytes=0-1023` → 1024; no header → 0. */
        fun nextFromRange(range: String?): Long =
            range?.substringAfterLast('-')?.trim()?.toLongOrNull()?.plus(1) ?: 0L
    }
}
