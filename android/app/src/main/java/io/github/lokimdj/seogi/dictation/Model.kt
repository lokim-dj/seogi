package io.github.lokimdj.seogi.dictation

import android.content.Context
import java.io.File
import java.io.IOException
import java.io.InputStream
import java.io.RandomAccessFile
import java.security.MessageDigest

/**
 * The speech model: Qwen3-ASR 0.6B, Q8_0 (Apache 2.0). The same Hugging Face
 * repository, revision and file as the desktop's catalog
 * (`src-tauri/src/catalog/catalog.json`; a test keeps the two equal). Never in
 * the APK: downloaded on first use, after asking, into the app's own storage.
 */
object DictationModel {
    const val REPOSITORY = "handy-computer/Qwen3-ASR-0.6B-gguf"
    const val REVISION = "e4e16599b900eb0cb36e524514756bb92eb092b7"
    const val FILE_NAME = "Qwen3-ASR-0.6B-Q8_0.gguf"
    const val SIZE = 850_423_456L
    const val SHA256 = "f081b2d5e23bd669d92cc331d722a8a0681943b8e6f34b48996fd5c319b5acd8"
    const val URL = "https://huggingface.co/$REPOSITORY/resolve/$REVISION/$FILE_NAME"

    /** Room to leave beside the model, so downloading it never fills the phone. */
    const val SPARE_BYTES = 300L * 1024 * 1024

    fun dir(context: Context) = File(context.filesDir, "models")
    fun file(context: Context) = File(dir(context), FILE_NAME)
    fun partFile(context: Context) = File(dir(context), "$FILE_NAME.part")

    /** The model is there whole. It is checked by SHA-256 before it gets its name, so the size is enough here. */
    fun isReady(context: Context) = file(context).length() == SIZE

    /** Bytes still to download, counting what an earlier try left. */
    fun remaining(context: Context): Long = SIZE - partFile(context).length().coerceIn(0, SIZE)
}

/** One answer to a ranged GET: the status and the body (closed by the caller). */
class Fetched(val code: Int, val body: InputStream) : AutoCloseable {
    override fun close() = body.close()
}

/** A GET from byte [from] on (a Range header when it is above 0). Throws [IOException] when the network fails. */
fun interface RangeFetch {
    fun get(url: String, from: Long): Fetched
}

/**
 * Downloads one file, continuing an earlier try where it stopped, and gives
 * it its name only after the size and SHA-256 are right. A wrong file is
 * deleted so the next try starts clean.
 */
class ModelDownloader(
    private val fetch: RangeFetch,
    private val url: String,
    private val size: Long,
    private val sha256: String,
) {
    sealed interface Result {
        data object Done : Result
        data object Stopped : Result
        /** The server sent something that is not the file (a changed revision, a bad copy). */
        data class Wrong(val why: String) : Result
        /** The server says the file is not there or not for us (404, 403…): trying again will not help. */
        data class Unavailable(val code: Int) : Result
    }

    /** Throws [IOException] when the network drops; what came so far stays in [part]. */
    fun download(part: File, dest: File, onProgress: (Long) -> Unit, stopped: () -> Boolean): Result {
        dest.parentFile?.mkdirs()
        if (dest.length() == size) return Result.Done
        var have = part.length()
        if (have > size) {
            part.delete()
            have = 0
        }
        if (have < size) {
            fetch.get(url, have).use { r ->
                when (r.code) {
                    206 -> {}
                    200 -> have = 0 // the server ignored the range: start over
                    416 -> {
                        part.delete()
                        return Result.Wrong("range not satisfiable at $have")
                    }
                    408, 429 -> throw IOException("HTTP ${r.code}") // busy: later
                    in 400..499 -> return Result.Unavailable(r.code)
                    else -> throw IOException("HTTP ${r.code}")
                }
                RandomAccessFile(part, "rw").use { out ->
                    out.setLength(have)
                    out.seek(have)
                    val buf = ByteArray(256 * 1024)
                    while (true) {
                        if (stopped()) return Result.Stopped
                        val n = r.body.read(buf)
                        if (n < 0) break
                        have += n
                        if (have > size) {
                            out.close()
                            part.delete()
                            return Result.Wrong("longer than $size")
                        }
                        out.write(buf, 0, n)
                        onProgress(have)
                    }
                }
            }
        }
        if (have < size) throw IOException("ended early at $have of $size")
        val got = sha256Of(part)
        if (!got.equals(sha256, ignoreCase = true)) {
            part.delete()
            return Result.Wrong("SHA-256 $got")
        }
        if (!part.renameTo(dest)) throw IOException("could not rename ${part.name}")
        return Result.Done
    }

    companion object {
        fun sha256Of(f: File): String {
            val md = MessageDigest.getInstance("SHA-256")
            f.inputStream().use { input ->
                val buf = ByteArray(1024 * 1024)
                while (true) {
                    val n = input.read(buf)
                    if (n < 0) break
                    md.update(buf, 0, n)
                }
            }
            return md.digest().joinToString("") { "%02x".format(it) }
        }
    }
}
