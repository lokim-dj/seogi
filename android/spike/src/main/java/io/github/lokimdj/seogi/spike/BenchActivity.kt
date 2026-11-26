package io.github.lokimdj.seogi.spike

import android.Manifest
import android.annotation.SuppressLint
import android.content.pm.PackageManager
import android.media.AudioFormat
import android.media.AudioRecord
import android.media.MediaRecorder
import android.os.Bundle
import android.util.Log
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material3.Button
import androidx.compose.material3.Checkbox
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import java.io.File
import java.util.concurrent.Executors

/**
 * Throwaway test screen: which on-phone speech engine is fast and right
 * enough in Korean. Not part of the app; see android/spike/README.md.
 */
class BenchActivity : ComponentActivity() {
    private val worker = Executors.newSingleThreadExecutor()
    private val lines = mutableStateListOf<String>()
    private var busy by mutableStateOf(false)
    private var partial by mutableStateOf("")
    private var recording by mutableStateOf(false)
    private val picked = mutableStateMapOf<String, Boolean>()
    private var noisy by mutableStateOf(false)

    private val base by lazy { getExternalFilesDir(null)!! }
    private val models get() = File(base, "models")
    private val clipsDir get() = File(base, "clips")
    private val results get() = File(base, "results")

    private var qwenThreads = 4
    private var zipThreads = 2

    private fun log(s: String) {
        Log.i(TAG, s)
        runOnUiThread { lines += s }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // A long run must not be slowed by the screen going off (no phone setting is changed).
        window.addFlags(android.view.WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        qwenThreads = intent.getIntExtra("qwenThreads", 4)
        zipThreads = intent.getIntExtra("zipThreads", 2)
        listOf(models, clipsDir, results).forEach { it.mkdirs() }
        val found = findEngines(models, qwenThreads, zipThreads)
        found.forEach { picked[it.name] = true }
        log("모델 폴더: ${models.path}")
        log("찾은 엔진: ${found.joinToString { "${it.name} (${it.sizeBytes / 1_000_000} MB)" }.ifEmpty { "없음" }}")
        log("시험 음성: ${loadClips(clipsDir, setOf("clean", "noisy")).size}개 · 스레드 qwen $qwenThreads, zipformer $zipThreads")

        if (intent.getBooleanExtra("auto", false)) {
            val takes = (intent.getStringExtra("takes") ?: "clean").split(',').toSet()
            val only = intent.getStringExtra("engines")?.split(',')
            val paced = intent.getBooleanExtra("paced", true)
            runFiles(takes, only, paced)
        }

        setContent {
            MaterialTheme(colorScheme = darkColorScheme()) {
                Column(Modifier.fillMaxSize().background(Color(0xFF16130F)).safeDrawingPadding().padding(16.dp)) {
                    Text("받아쓰기 엔진 시험 (시제품)", fontSize = 20.sp, color = Color.White)
                    picked.keys.sorted().forEach { name ->
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Checkbox(picked[name] == true, { picked[name] = it }, enabled = !busy)
                            Text(name, color = Color.White, fontSize = 14.sp)
                        }
                    }
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Checkbox(noisy, { noisy = it }, enabled = !busy)
                        Text("잡음 섞인 음성도", color = Color.White, fontSize = 14.sp)
                    }
                    Button(onClick = {
                        runFiles(if (noisy) setOf("clean", "noisy") else setOf("clean"), picked.filterValues { it }.keys.toList(), true)
                    }, enabled = !busy && !recording, modifier = Modifier.fillMaxWidth()) { Text("파일로 시험 → 결과 저장") }
                    Spacer(Modifier.height(8.dp))
                    Box(
                        Modifier.fillMaxWidth().height(96.dp)
                            .background(if (recording) Color(0xFFB3261E) else Color(0xFF3A342C))
                            .pointerInput(busy) {
                                detectTapGestures(onPress = {
                                    if (busy) return@detectTapGestures
                                    if (startLive()) { tryAwaitRelease(); stopLive() }
                                })
                            },
                        contentAlignment = Alignment.Center,
                    ) {
                        Text(if (recording) "말하는 중… 손을 떼면 끝" else "누르고 말하기 (떼면 끝)", color = Color.White, fontSize = 18.sp)
                    }
                    if (partial.isNotEmpty()) Text(partial, color = Color(0xFFE8DCC8), fontSize = 18.sp, modifier = Modifier.padding(top = 8.dp))
                    val state = rememberLazyListState()
                    LaunchedEffect(lines.size) { if (lines.isNotEmpty()) state.scrollToItem(lines.size - 1) }
                    LazyColumn(Modifier.fillMaxWidth().weight(1f).padding(top = 8.dp), state) {
                        items(lines) { Text(it, color = Color(0xFFBDB3A3), fontSize = 12.sp) }
                    }
                }
            }
        }
    }

    private fun runFiles(takes: Set<String>, only: List<String>?, paced: Boolean) {
        busy = true
        worker.execute {
            try {
                releaseLive()
                val clips = loadClips(clipsDir, takes)
                val engines = findEngines(models, qwenThreads, zipThreads).filter { e -> only == null || only.any { e.name.contains(it) } }
                log("파일 시험: 엔진 ${engines.size}개 × 음성 ${clips.size}개 (${if (paced) "말하는 속도로" else "한꺼번에"})")
                val bench = Bench(this, ::log)
                bench.runFiles(engines, clips, paced)
                val csv = bench.save(results)
                for (s in bench.summary()) {
                    log("■ ${s["engine"]} [${s["mode"]}] 중앙 ${fmt(s["median_latency_ms"])} ms · p90 ${fmt(s["p90_latency_ms"])} ms · CER ${fmt(s["mean_cer"], 3)} · RTF ${fmt(s["mean_rtf"], 3)} · ${fmt(s["size_mb"])} MB · 최대 RSS ${fmt(s["peak_rss_mb"])} MB")
                }
                log("저장: ${csv.path}")
                Log.i(TAG, "BENCH DONE ${csv.path}")
            } catch (e: Throwable) {
                log("실패: $e"); Log.e(TAG, "BENCH FAILED", e)
            } finally {
                runOnUiThread { busy = false }
            }
        }
    }

    // --- live microphone: press, speak, release ---
    private var live: List<Engine> = emptyList()
    private var record: AudioRecord? = null
    private var reader: Thread? = null
    private val captured = java.io.ByteArrayOutputStream()
    private var liveBench: Bench? = null

    private fun releaseLive() {
        live.forEach { it.close() }
        live = emptyList()
    }

    @SuppressLint("MissingPermission")
    private fun startLive(): Boolean {
        if (checkSelfPermission(Manifest.permission.RECORD_AUDIO) != PackageManager.PERMISSION_GRANTED) {
            requestPermissions(arrayOf(Manifest.permission.RECORD_AUDIO), 1)
            return false
        }
        val want = picked.filterValues { it }.keys
        if (live.map { it.name }.toSet() != want) {
            log("실시간 준비: 엔진 불러오는 중… 끝나면 다시 누르세요")
            busy = true
            worker.execute {
                releaseLive()
                live = findEngines(models, qwenThreads, zipThreads).filter { it.name in want }
                    .onEach { e -> val t = System.nanoTime(); e.load(); log("  ${e.name} 불러옴 ${"%.0f".format((System.nanoTime() - t) / 1e6)} ms") }
                log("준비됨: 누르고 말해 보세요")
                runOnUiThread { busy = false }
            }
            return false
        }
        val min = AudioRecord.getMinBufferSize(RATE, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT)
        val rec = AudioRecord(MediaRecorder.AudioSource.VOICE_RECOGNITION, RATE, AudioFormat.CHANNEL_IN_MONO,
            AudioFormat.ENCODING_PCM_16BIT, maxOf(min, RATE))
        record = rec
        captured.reset()
        partial = ""
        val streaming = live.filterIsInstance<Live>()
        streaming.forEach { it.begin { t -> runOnUiThread { partial = t } } }
        rec.startRecording()
        recording = true
        reader = Thread {
            val buf = ShortArray(RATE / 10)
            while (recording) {
                val n = rec.read(buf, 0, buf.size)
                if (n <= 0) continue
                val f = FloatArray(n) { buf[it] / 32768f }
                for (i in 0 until n) { captured.write(buf[i].toInt() and 0xff); captured.write(buf[i].toInt() shr 8 and 0xff) }
                streaming.forEach { it.feed(f) }
            }
        }.apply { start() }
        return true
    }

    private fun stopLive() {
        recording = false
        val rec = record ?: return
        record = null
        worker.execute {
            reader?.join()
            rec.stop(); rec.release()
            val bytes = captured.toByteArray()
            val pcm = FloatArray(bytes.size / 2) { ((bytes[2 * it].toInt() and 0xff) or (bytes[2 * it + 1].toInt() shl 8)).toShort() / 32768f }
            val seconds = pcm.size.toDouble() / RATE
            val bench = liveBench ?: Bench(this, ::log).also { liveBench = it }
            log("— 말한 길이 ${"%.1f".format(seconds)} s")
            for (e in live) {
                val o = if (e is Live) e.end() else e.run(pcm, paced = false)
                bench.addLive(e.name, seconds, o, "")
                log("  ${e.name}: 뗀 뒤 ${"%.0f".format(o.endLatencyMs)} ms${o.firstPartialMs?.let { " · 첫 글자 ${"%.0f".format(it)} ms" } ?: ""}\n    ${o.text}${o.error?.let { " [$it]" } ?: ""}")
            }
            log("저장: ${bench.save(results).path}")
        }
    }

    override fun onDestroy() {
        recording = false
        worker.execute { releaseLive() }
        worker.shutdown()
        super.onDestroy()
    }

    private fun fmt(v: Any?, digits: Int = 0): String = (v as? Number)?.let { "%.${digits}f".format(it.toDouble()) } ?: "-"

    companion object { const val TAG = "SeogiBench" }
}
