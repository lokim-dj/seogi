package io.github.lokimdj.seogi.dictation

import java.io.File

/** The engine's native part, `libseogi_asr.so` (transcribe.cpp, MIT). */
internal object TranscribeCpp {
    @JvmStatic external fun nativeOpen(path: String, threads: Int): Long
    @JvmStatic external fun nativeRun(handle: Long, pcm: FloatArray, language: String?): ByteArray
    @JvmStatic external fun nativeClose(handle: Long)
}

/**
 * Qwen3-ASR through transcribe.cpp, on the phone's CPU. One utterance at a
 * time, after the speaking has ended. Not thread-safe: [Dictator] uses it from
 * one thread.
 */
class SpeechEngine private constructor(private var handle: Long) : AutoCloseable {

    /** The text of [pcm] (16 kHz mono floats), or throws with the engine's reason. */
    fun transcribe(pcm: FloatArray): String {
        check(handle != 0L) { "engine closed" }
        val raw = String(TranscribeCpp.nativeRun(handle, pcm, "ko"), Charsets.UTF_8)
        if (raw.startsWith("\u0001")) throw IllegalStateException(raw.drop(1))
        return raw.trim()
    }

    override fun close() {
        if (handle != 0L) TranscribeCpp.nativeClose(handle)
        handle = 0L
    }

    companion object {
        const val RATE = 16_000

        /** Threads for the 0.6B model: the measured setting on the phone (spike, 11/27). */
        private const val THREADS = 4

        @Volatile private var loaded: Boolean? = null

        /**
         * The native library is built for arm64 with dot product and int8
         * matrix instructions. Loading it on a CPU without them would crash the
         * process at the first sum, so the CPU is asked first.
         */
        fun available(): Boolean {
            loaded?.let { return it }
            val ok = cpuSupportedHere() && try {
                System.loadLibrary("seogi_asr")
                true
            } catch (e: Throwable) {
                false
            }
            loaded = ok
            return ok
        }

        /** Opens the model file; slow (a second or two) and about 1.4 GB of memory. */
        fun open(model: File): SpeechEngine {
            check(available()) { "this phone's CPU cannot run the engine" }
            val h = TranscribeCpp.nativeOpen(model.path, THREADS)
            check(h != 0L) { "could not open ${model.name}" }
            return SpeechEngine(h)
        }

        /** The check alone, without loading the library (for the settings screen): a 64-bit ARM app with the instructions. */
        fun cpuSupportedHere(): Boolean =
            "arm64-v8a" in android.os.Build.SUPPORTED_64_BIT_ABIS && cpuSupported(readCpuFeatures())

        private fun readCpuFeatures(): String =
            try { File("/proc/cpuinfo").readText() } catch (e: Exception) { "" }

        /** True when every core's "Features" line has `asimddp` (dot product) and `i8mm`. */
        fun cpuSupported(cpuinfo: String): Boolean {
            val lines = cpuinfo.lines().filter { it.trimStart().startsWith("Features") }
            if (lines.isEmpty()) return false
            return lines.all { line ->
                val words = line.substringAfter(':').trim().split(Regex("\\s+")).toSet()
                "asimddp" in words && "i8mm" in words
            }
        }
    }
}
