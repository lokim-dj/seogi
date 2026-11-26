package io.github.lokimdj.seogi.spike

import java.io.File
import java.nio.ByteBuffer
import java.nio.ByteOrder

/** 16-bit mono PCM WAV at 16 kHz to floats in [-1, 1]. */
fun readWav16k(file: File): FloatArray {
    val bytes = file.readBytes()
    val bb = ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN)
    var pos = 12
    var rate = 0
    var channels = 0
    var bits = 0
    while (pos + 8 <= bytes.size) {
        val id = String(bytes, pos, 4, Charsets.US_ASCII)
        val size = bb.getInt(pos + 4)
        if (id == "fmt ") {
            channels = bb.getShort(pos + 10).toInt()
            rate = bb.getInt(pos + 12)
            bits = bb.getShort(pos + 22).toInt()
        } else if (id == "data") {
            require(rate == 16_000 && channels == 1 && bits == 16) { "${file.name}: need 16 kHz mono 16-bit, got $rate/$channels/$bits" }
            val n = minOf(size, bytes.size - pos - 8) / 2
            return FloatArray(n) { bb.getShort(pos + 8 + it * 2) / 32768f }
        }
        pos += 8 + size + (size and 1)
    }
    error("${file.name}: no data chunk")
}

/**
 * Character error rate on the letters only: spaces and punctuation are
 * dropped on both sides (Zipformer writes no punctuation, Qwen3 does), so the
 * number says how many syllables were wrong. Digits are left as they are, so
 * "세 시" against "3시" counts as wrong; the raw text is kept for a look.
 */
fun cer(reference: String, hypothesis: String): Double {
    val r = normalize(reference)
    val h = normalize(hypothesis)
    if (r.isEmpty()) return if (h.isEmpty()) 0.0 else 1.0
    return levenshtein(r, h).toDouble() / r.length
}

fun normalize(s: String): String =
    s.lowercase().filter { it.isLetterOrDigit() }

fun levenshtein(a: String, b: String): Int {
    var prev = IntArray(b.length + 1) { it }
    var cur = IntArray(b.length + 1)
    for (i in 1..a.length) {
        cur[0] = i
        for (j in 1..b.length) {
            val cost = if (a[i - 1] == b[j - 1]) 0 else 1
            cur[j] = minOf(prev[j] + 1, cur[j - 1] + 1, prev[j - 1] + cost)
        }
        val t = prev; prev = cur; cur = t
    }
    return prev[b.length]
}

fun percentile(values: List<Double>, p: Double): Double {
    if (values.isEmpty()) return Double.NaN
    val s = values.sorted()
    val rank = p * (s.size - 1)
    val lo = rank.toInt()
    val hi = minOf(lo + 1, s.size - 1)
    return s[lo] + (s[hi] - s[lo]) * (rank - lo)
}

/** VmRSS and RssAnon of this process in MB, from /proc/self/status. */
fun memoryMb(): Pair<Double, Double> {
    var rss = 0.0
    var anon = 0.0
    try {
        File("/proc/self/status").forEachLine { line ->
            if (line.startsWith("VmRSS:")) rss = line.filter { it.isDigit() }.toDouble() / 1024
            if (line.startsWith("RssAnon:")) anon = line.filter { it.isDigit() }.toDouble() / 1024
        }
    } catch (_: Exception) {
    }
    return rss to anon
}

/** Samples memory every 20 ms while a block runs; reports the peaks. */
class MemPeak : AutoCloseable {
    @Volatile var peakRss = 0.0
    @Volatile var peakAnon = 0.0
    @Volatile private var running = true
    private val thread = Thread {
        while (running) {
            val (r, a) = memoryMb()
            if (r > peakRss) peakRss = r
            if (a > peakAnon) peakAnon = a
            Thread.sleep(20)
        }
    }.apply { isDaemon = true; start() }

    override fun close() {
        running = false
        thread.join(200)
        val (r, a) = memoryMb()
        if (r > peakRss) peakRss = r
        if (a > peakAnon) peakAnon = a
    }
}
