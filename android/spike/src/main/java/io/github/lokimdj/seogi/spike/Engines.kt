package io.github.lokimdj.seogi.spike

import com.k2fsa.sherpa.onnx.EndpointConfig
import com.k2fsa.sherpa.onnx.FeatureConfig
import com.k2fsa.sherpa.onnx.OnlineModelConfig
import com.k2fsa.sherpa.onnx.OnlineRecognizer
import com.k2fsa.sherpa.onnx.OnlineRecognizerConfig
import com.k2fsa.sherpa.onnx.OnlineStream
import com.k2fsa.sherpa.onnx.OnlineTransducerModelConfig
import java.io.File

const val RATE = 16_000

/** One utterance's outcome. Times in ms, measured with System.nanoTime. */
data class Outcome(
    val text: String,
    /** From the last sample of speech handed over to the final text. */
    val endLatencyMs: Double,
    /** Time spent computing for the whole utterance. */
    val computeMs: Double,
    /** From the first sample to the first non-empty partial text (streaming only). */
    val firstPartialMs: Double? = null,
    val error: String? = null,
)

interface Engine : AutoCloseable {
    val name: String
    val sizeBytes: Long
    fun load()

    /**
     * Transcribe [pcm]. With [paced] the audio is handed over at the speed it
     * would come from the microphone (a streaming engine works while it
     * arrives); without it, all at once.
     */
    fun run(pcm: FloatArray, paced: Boolean, onPartial: (String) -> Unit = {}): Outcome
}

/** Text appears while speaking; at the end only the last bit is left to compute. */
interface Live {
    fun begin(onPartial: (String) -> Unit)
    fun feed(samples: FloatArray)
    fun end(): Outcome
}

/** sherpa-onnx (Apache 2.0) with the Korean streaming Zipformer (Apache 2.0). */
class ZipformerEngine(private val dir: File, private val threads: Int = 2) : Engine, Live {
    override val name = "zipformer-ko-int8"
    private fun pick(prefix: String, int8: Boolean): File =
        dir.listFiles().orEmpty().filter { it.name.startsWith(prefix) && it.name.endsWith(".onnx") }
            .sortedBy { if (it.name.contains(".int8.") == int8) 0 else 1 }.firstOrNull()
            ?: error("missing $prefix*.onnx in $dir")

    private val encoder by lazy { pick("encoder", true) }
    private val decoder by lazy { pick("decoder", false) }
    private val joiner by lazy { pick("joiner", true) }
    override val sizeBytes: Long
        get() = listOf(encoder, decoder, joiner, File(dir, "tokens.txt")).sumOf { it.length() }

    private var recognizer: OnlineRecognizer? = null

    override fun load() {
        val config = OnlineRecognizerConfig(
            featConfig = FeatureConfig(sampleRate = RATE, featureDim = 80),
            modelConfig = OnlineModelConfig(
                transducer = OnlineTransducerModelConfig(encoder.path, decoder.path, joiner.path),
                tokens = File(dir, "tokens.txt").path,
                numThreads = threads,
                modelType = "zipformer2",
            ),
            endpointConfig = EndpointConfig(),
            enableEndpoint = false,
            decodingMethod = "greedy_search",
        )
        recognizer = OnlineRecognizer(null, config)
    }

    // --- live use ---
    private var stream: OnlineStream? = null
    private var computeNs = 0L
    private var startNs = 0L
    private var firstPartialNs = 0L
    private var partialCb: (String) -> Unit = {}
    private var lastText = ""

    override fun begin(onPartial: (String) -> Unit) {
        stream?.release()
        stream = recognizer!!.createStream()
        computeNs = 0; firstPartialNs = 0; lastText = ""
        startNs = System.nanoTime()
        partialCb = onPartial
    }

    override fun feed(samples: FloatArray) {
        val r = recognizer!!
        val s = stream!!
        val t0 = System.nanoTime()
        s.acceptWaveform(samples, RATE)
        while (r.isReady(s)) r.decode(s)
        val text = r.getResult(s).text
        computeNs += System.nanoTime() - t0
        if (text != lastText) {
            lastText = text
            if (text.isNotBlank() && firstPartialNs == 0L) firstPartialNs = System.nanoTime()
            partialCb(text)
        }
    }

    override fun end(): Outcome {
        val r = recognizer!!
        val s = stream!!
        val t0 = System.nanoTime()
        // A little silence flushes the encoder's look-ahead; it is computed, not waited for.
        s.acceptWaveform(FloatArray(RATE * 3 / 10), RATE)
        s.inputFinished()
        while (r.isReady(s)) r.decode(s)
        val text = r.getResult(s).text.trim()
        val t1 = System.nanoTime()
        computeNs += t1 - t0
        s.release(); stream = null
        return Outcome(
            text = text,
            endLatencyMs = (t1 - t0) / 1e6,
            computeMs = computeNs / 1e6,
            firstPartialMs = if (firstPartialNs == 0L) null else (firstPartialNs - startNs) / 1e6,
        )
    }

    override fun run(pcm: FloatArray, paced: Boolean, onPartial: (String) -> Unit): Outcome {
        begin(onPartial)
        val chunk = RATE / 10 // 100 ms, like a microphone buffer
        var i = 0
        val wall0 = System.nanoTime()
        while (i < pcm.size) {
            val n = minOf(chunk, pcm.size - i)
            feed(pcm.copyOfRange(i, i + n))
            i += n
            if (paced) {
                val due = wall0 + i * 1_000_000_000L / RATE
                val wait = due - System.nanoTime()
                if (wait > 0) Thread.sleep(wait / 1_000_000, (wait % 1_000_000).toInt())
            }
        }
        return end()
    }

    override fun close() {
        stream?.release(); stream = null
        recognizer?.release(); recognizer = null
    }
}

/** Qwen3-ASR through transcribe.cpp (MIT), the desktop's engine. Whole utterance at the end. */
class QwenEngine(private val gguf: File, private val threads: Int = 4) : Engine {
    override val name = "qwen3-asr-0.6b-" + gguf.nameWithoutExtension.substringAfterLast('-').lowercase()
    override val sizeBytes get() = gguf.length()
    private var handle = 0L

    override fun load() {
        handle = TranscribeCpp.nativeOpen(gguf.path, threads)
        check(handle != 0L) { "transcribe.cpp could not open ${gguf.name} (see logcat SeogiAsr)" }
    }

    override fun run(pcm: FloatArray, paced: Boolean, onPartial: (String) -> Unit): Outcome {
        // Nothing can start before the speech has ended, so pacing changes nothing.
        val t0 = System.nanoTime()
        val raw = String(TranscribeCpp.nativeRun(handle, pcm, "ko"), Charsets.UTF_8)
        val ms = (System.nanoTime() - t0) / 1e6
        if (raw.startsWith("\u0001")) return Outcome("", ms, ms, error = raw.drop(1))
        return Outcome(raw.trim(), ms, ms)
    }

    override fun close() {
        if (handle != 0L) TranscribeCpp.nativeClose(handle)
        handle = 0L
    }
}

object TranscribeCpp {
    init { System.loadLibrary("seogi_asr") }
    @JvmStatic external fun nativeOpen(path: String, threads: Int): Long
    @JvmStatic external fun nativeRun(handle: Long, pcm: FloatArray, language: String?): ByteArray
    @JvmStatic external fun nativeClose(handle: Long)
}

/** Every engine whose model files are on the phone, under files/models/. */
fun findEngines(models: File, qwenThreads: Int, zipThreads: Int): List<Engine> {
    val out = mutableListOf<Engine>()
    val zip = File(models, "zipformer-ko")
    if (File(zip, "tokens.txt").exists()) out += ZipformerEngine(zip, zipThreads)
    File(models, "qwen3").listFiles().orEmpty().filter { it.name.endsWith(".gguf") }.sortedBy { it.length() }
        .forEach { out += QwenEngine(it, qwenThreads) }
    return out
}
