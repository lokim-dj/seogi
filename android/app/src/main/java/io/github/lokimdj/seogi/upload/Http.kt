package io.github.lokimdj.seogi.upload

import java.io.IOException
import java.net.HttpURLConnection
import java.net.URL

/** One HTTP answer; header names are lower case. */
data class HttpResponse(val code: Int, val headers: Map<String, String>, val body: String)

/** The little HTTP the Drive calls need, so tests can stand in for the network. */
fun interface Http {
    /** Throws [IOException] when the network fails; any status code is returned. */
    fun send(method: String, url: String, headers: Map<String, String>, body: ByteArray?): HttpResponse
}

/** [Http] on the platform's HttpURLConnection. */
object UrlConnectionHttp : Http {
    override fun send(method: String, url: String, headers: Map<String, String>, body: ByteArray?): HttpResponse {
        val c = URL(url).openConnection() as HttpURLConnection
        try {
            c.instanceFollowRedirects = false
            c.connectTimeout = 20_000
            c.readTimeout = 60_000
            c.requestMethod = method
            headers.forEach { (k, v) -> c.setRequestProperty(k, v) }
            if (body != null) {
                c.doOutput = true
                c.setFixedLengthStreamingMode(body.size)
                c.outputStream.use { it.write(body) }
            }
            val code = c.responseCode
            val text = (if (code >= 400) c.errorStream else c.inputStream)?.use { it.readBytes().decodeToString() }.orEmpty()
            val hs = c.headerFields.filterKeys { it != null }.mapKeys { it.key.lowercase() }.mapValues { it.value.joinToString(",") }
            return HttpResponse(code, hs, text)
        } finally {
            c.disconnect()
        }
    }
}
