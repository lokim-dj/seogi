package io.github.lokimdj.seogi.spike

import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.os.BatteryManager
import android.os.Build
import android.os.PowerManager
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

data class Clip(val file: File, val text: String, val seconds: Double)

data class Row(
    val engine: String, val mode: String, val clip: String, val seconds: Double,
    val reference: String, val text: String, val endLatencyMs: Double, val computeMs: Double,
    val firstPartialMs: Double?, val cer: Double?, val error: String?,
) {
    val rtf get() = computeMs / 1000 / seconds
}

data class EngineSummary(
    val engine: String, val sizeMb: Double, val loadMs: Double, val rssBeforeMb: Double,
    val peakRssMb: Double, val peakAnonMb: Double, val tempStartC: Double, val tempEndC: Double,
    val thermalStart: Int, val thermalEnd: Int, val chargeUahDelta: Long,
)

fun loadClips(dir: File, takes: Set<String>): List<Clip> {
    val manifest = File(dir, "clips.json")
    if (!manifest.exists()) return emptyList()
    val arr = JSONArray(manifest.readText())
    return (0 until arr.length()).map { arr.getJSONObject(it) }
        .filter { o -> takes.any { o.getString("file").startsWith(it) } }
        .map { Clip(File(dir, it.getString("file")), it.getString("text"), it.getDouble("seconds")) }
}

class PhoneState(private val ctx: Context) {
    fun tempC(): Double {
        val i = ctx.registerReceiver(null, IntentFilter(Intent.ACTION_BATTERY_CHANGED))
        return (i?.getIntExtra(BatteryManager.EXTRA_TEMPERATURE, 0) ?: 0) / 10.0
    }
    fun thermal(): Int = (ctx.getSystemService(Context.POWER_SERVICE) as PowerManager).currentThermalStatus
    fun chargeUah(): Long =
        (ctx.getSystemService(Context.BATTERY_SERVICE) as BatteryManager)
            .getLongProperty(BatteryManager.BATTERY_PROPERTY_CHARGE_COUNTER)
}

/**
 * Runs every clip through each engine, one engine loaded at a time. The first
 * clip is run once untimed first (warm-up), as the real feature would keep the
 * engine loaded.
 */
class Bench(private val ctx: Context, private val log: (String) -> Unit) {
    val rows = mutableListOf<Row>()
    val summaries = mutableListOf<EngineSummary>()
    private val phone = PhoneState(ctx)

    fun runFiles(engines: List<Engine>, clips: List<Clip>, paced: Boolean) {
        for (engine in engines) {
            log("— ${engine.name}: 불러오는 중")
            System.gc()
            val (rssBefore, _) = memoryMb()
            val temp0 = phone.tempC(); val th0 = phone.thermal(); val ch0 = phone.chargeUah()
            var loadMs = 0.0
            val peak = MemPeak()
            try {
                val t0 = System.nanoTime()
                engine.load()
                loadMs = (System.nanoTime() - t0) / 1e6
                log("  불러옴 ${"%.0f".format(loadMs)} ms")
                clips.firstOrNull()?.let { engine.run(readWav16k(it.file), paced = false) }
                for (clip in clips) {
                    val pcm = readWav16k(clip.file)
                    val o = try { engine.run(pcm, paced) } catch (e: Throwable) { Outcome("", 0.0, 0.0, error = e.toString()) }
                    val row = Row(engine.name, if (paced) "file-paced" else "file", clip.file.name, clip.seconds,
                        clip.text, o.text, o.endLatencyMs, o.computeMs, o.firstPartialMs,
                        if (o.error == null) cer(clip.text, o.text) else null, o.error)
                    rows += row
                    log("  ${clip.file.name} ${"%.0f".format(o.endLatencyMs)} ms · CER ${row.cer?.let { "%.2f".format(it) } ?: "-"} · ${o.text.take(40)}")
                }
            } catch (e: Throwable) {
                log("  실패: $e")
                rows += Row(engine.name, "load", "-", 0.0, "", "", 0.0, 0.0, null, null, e.toString())
            } finally {
                engine.close()
                peak.close()
            }
            summaries += EngineSummary(engine.name, engine.sizeBytes / 1e6, loadMs, rssBefore, peak.peakRss,
                peak.peakAnon, temp0, phone.tempC(), th0, phone.thermal(), phone.chargeUah() - ch0)
        }
    }

    fun addLive(engine: String, seconds: Double, o: Outcome, reference: String) {
        rows += Row(engine, "live", "mic", seconds, reference, o.text, o.endLatencyMs, o.computeMs,
            o.firstPartialMs, if (reference.isNotBlank() && o.error == null) cer(reference, o.text) else null, o.error)
    }

    /** Writes results/<time>.csv (one row per utterance) and .json (rows, engines, summary). */
    fun save(dir: File): File {
        dir.mkdirs()
        val stamp = SimpleDateFormat("yyyyMMdd-HHmmss", Locale.US).format(Date())
        val csv = File(dir, "bench-$stamp.csv")
        csv.bufferedWriter().use { w ->
            w.write("engine,mode,clip,seconds,end_latency_ms,compute_ms,rtf,first_partial_ms,cer,reference,text,error\n")
            for (r in rows) {
                w.write(listOf(r.engine, r.mode, r.clip, "%.2f".format(r.seconds), "%.1f".format(r.endLatencyMs),
                    "%.1f".format(r.computeMs), "%.3f".format(r.rtf), r.firstPartialMs?.let { "%.0f".format(it) } ?: "",
                    r.cer?.let { "%.4f".format(it) } ?: "", r.reference, r.text, r.error ?: "")
                    .joinToString(",") { csvCell(it) } + "\n")
            }
        }
        val json = JSONObject()
        json.put("device", "${Build.MANUFACTURER} ${Build.MODEL} (Android ${Build.VERSION.RELEASE}, SoC ${Build.SOC_MODEL})")
        json.put("summary", JSONArray(summary().map { JSONObject(it) }))
        json.put("engines", JSONArray(summaries.map { s ->
            JSONObject().put("engine", s.engine).put("size_mb", s.sizeMb).put("load_ms", s.loadMs)
                .put("rss_before_mb", s.rssBeforeMb).put("peak_rss_mb", s.peakRssMb).put("peak_anon_mb", s.peakAnonMb)
                .put("battery_temp_c", "${s.tempStartC} -> ${s.tempEndC}").put("thermal_status", "${s.thermalStart} -> ${s.thermalEnd}")
                .put("charge_uah_delta", s.chargeUahDelta)
        }))
        json.put("rows", JSONArray(rows.map { r ->
            JSONObject().put("engine", r.engine).put("mode", r.mode).put("clip", r.clip).put("seconds", r.seconds)
                .put("end_latency_ms", r.endLatencyMs).put("compute_ms", r.computeMs).put("rtf", r.rtf)
                .put("first_partial_ms", r.firstPartialMs ?: JSONObject.NULL).put("cer", r.cer ?: JSONObject.NULL)
                .put("reference", r.reference).put("text", r.text).put("error", r.error ?: JSONObject.NULL)
        }))
        File(dir, "bench-$stamp.json").writeText(json.toString(1))
        return csv
    }

    /** Per engine and mode: median and p90 end latency, mean CER, mean RTF. */
    fun summary(): List<Map<String, Any?>> =
        rows.filter { it.error == null && it.mode != "load" }.groupBy { it.engine to it.mode }.map { (k, rs) ->
            val lat = rs.map { it.endLatencyMs }
            val cers = rs.mapNotNull { it.cer }
            val s = summaries.firstOrNull { it.engine == k.first }
            mapOf(
                "engine" to k.first, "mode" to k.second, "n" to rs.size,
                "median_latency_ms" to percentile(lat, 0.5), "p90_latency_ms" to percentile(lat, 0.9),
                "mean_cer" to if (cers.isEmpty()) null else cers.average(),
                "mean_rtf" to rs.map { it.rtf }.average(),
                "median_first_partial_ms" to rs.mapNotNull { it.firstPartialMs }.let { if (it.isEmpty()) null else percentile(it, 0.5) },
                "size_mb" to s?.sizeMb, "peak_rss_mb" to s?.peakRssMb, "peak_anon_mb" to s?.peakAnonMb,
            )
        }

    private fun csvCell(s: String) = if (s.any { it == ',' || it == '"' || it == '\n' }) "\"" + s.replace("\"", "\"\"") + "\"" else s
}
