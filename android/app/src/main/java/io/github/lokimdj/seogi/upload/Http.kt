package io.github.lokimdj.seogi.upload

import okhttp3.MediaType.Companion.toMediaTypeOrNull
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import java.io.File
import java.io.IOException
import java.util.concurrent.TimeUnit

/** One HTTP answer; header names are lower case. */
data class HttpResponse(val code: Int, val headers: Map<String, String>, val body: String)

/** The little HTTP the Drive calls need, so tests can stand in for the network. */
fun interface Http {
    /** Throws [IOException] when the network fails; any status code is returned. */
    fun send(method: String, url: String, headers: Map<String, String>, body: ByteArray?): HttpResponse

    /**
     * GETs [url] into [to], asking only for what [to] does not hold yet.
     * Returns the status code; [to] is left as it is on any other than 2xx.
     * This plain form reads the whole answer as text (enough for tests).
     */
    fun download(url: String, headers: Map<String, String>, to: File, onProgress: (Long, Long) -> Unit): Int {
        val r = send("GET", url, headers, null)
        if (r.code in 200..299) {
            val bytes = r.body.toByteArray()
            to.writeBytes(bytes)
            onProgress(bytes.size.toLong(), bytes.size.toLong())
        }
        return r.code
    }
}

/**
 * [Http] on OkHttp, which speaks PATCH and DELETE (HttpURLConnection does not).
 * Redirects are not followed: Drive answers a resumable upload with 308.
 */
object OkHttpHttp : Http {
    private val client = OkHttpClient.Builder()
        .followRedirects(false)
        .followSslRedirects(false)
        .connectTimeout(20, TimeUnit.SECONDS)
        .readTimeout(60, TimeUnit.SECONDS)
        .writeTimeout(60, TimeUnit.SECONDS)
        .build()

    override fun send(method: String, url: String, headers: Map<String, String>, body: ByteArray?): HttpResponse {
        val needsBody = method in setOf("POST", "PUT", "PATCH")
        val type = headers.entries.firstOrNull { it.key.equals("Content-Type", true) }?.value?.toMediaTypeOrNull()
        val requestBody = (body ?: if (needsBody) ByteArray(0) else null)?.toRequestBody(type)
        val request = Request.Builder().url(url)
            .apply { headers.forEach { (k, v) -> header(k, v) } }
            .method(method, requestBody)
            .build()
        client.newCall(request).execute().use { r ->
            val hs = r.headers.toMultimap().mapKeys { it.key.lowercase() }.mapValues { it.value.joinToString(",") }
            return HttpResponse(r.code, hs, r.body?.string().orEmpty())
        }
    }

    override fun download(url: String, headers: Map<String, String>, to: File, onProgress: (Long, Long) -> Unit): Int {
        val have = if (to.exists()) to.length() else 0L
        val request = Request.Builder().url(url)
            .apply { headers.forEach { (k, v) -> header(k, v) } }
            .apply { if (have > 0) header("Range", "bytes=$have-") }
            .get()
            .build()
        client.newCall(request).execute().use { r ->
            if (r.code !in 200..299) return r.code
            val body = r.body ?: return r.code
            // 206: Drive sends the rest; 200: it sends it all again.
            val from = if (r.code == 206) have else 0L
            val total = body.contentLength().let { if (it >= 0) from + it else 0L }
            java.io.FileOutputStream(to, r.code == 206).use { out ->
                val buf = ByteArray(64 * 1024)
                var done = from
                body.byteStream().use { input ->
                    while (true) {
                        val n = input.read(buf)
                        if (n < 0) break
                        out.write(buf, 0, n)
                        done += n
                        onProgress(done, total)
                    }
                }
            }
            return r.code
        }
    }
}
