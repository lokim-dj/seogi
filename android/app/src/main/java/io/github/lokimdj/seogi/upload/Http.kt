package io.github.lokimdj.seogi.upload

import okhttp3.MediaType.Companion.toMediaTypeOrNull
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import java.io.IOException
import java.util.concurrent.TimeUnit

/** One HTTP answer; header names are lower case. */
data class HttpResponse(val code: Int, val headers: Map<String, String>, val body: String)

/** The little HTTP the Drive calls need, so tests can stand in for the network. */
fun interface Http {
    /** Throws [IOException] when the network fails; any status code is returned. */
    fun send(method: String, url: String, headers: Map<String, String>, body: ByteArray?): HttpResponse
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
}
