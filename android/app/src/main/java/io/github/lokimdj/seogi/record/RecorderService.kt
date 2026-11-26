package io.github.lokimdj.seogi.record

import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.media.MediaRecorder
import android.os.PowerManager
import android.os.StatFs
import android.os.SystemClock
import androidx.core.app.NotificationCompat
import androidx.core.content.ContextCompat
import androidx.lifecycle.LifecycleService
import androidx.lifecycle.lifecycleScope
import io.github.lokimdj.seogi.R
import io.github.lokimdj.seogi.SeogiApp
import io.github.lokimdj.seogi.data.FileNames
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.time.Instant
import java.time.ZoneId

/** What the recorder is doing, for the screens. */
sealed interface RecState {
    data object Idle : RecState

    data class Recording(
        val id: String,
        val title: String,
        val fromCalendar: Boolean,
        val startedAt: Long,
        /** [SystemClock.elapsedRealtime] at the start, for the running time. */
        val startedElapsed: Long,
        val marksMs: List<Long>,
    ) : RecState

    /** Stopped; the file is being saved to `Recordings/서기/`. */
    data object Saving : RecState
}

/**
 * Records the microphone in a foreground service, so it keeps going with the
 * screen off or another app in front. The AAC stream is written as ADTS,
 * which is readable up to its last frame even if the app is killed; the
 * finished file is made from it by [Finalizer].
 */
class RecorderService : LifecycleService() {
    private var recorder: MediaRecorder? = null
    private var wakeLock: PowerManager.WakeLock? = null
    private var ticker: Job? = null
    private lateinit var sessions: SessionDir

    override fun onCreate() {
        super.onCreate()
        sessions = SessionDir(sessionDir(this))
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        super.onStartCommand(intent, flags, startId)
        when (intent?.action) {
            ACTION_START -> if (_state.value == RecState.Idle) begin() else if (_state.value is RecState.Recording) showNotification()
            ACTION_MARK -> mark()
            ACTION_STOP -> finish()
            ACTION_DISCARD -> discard()
        }
        // A mark or stop that arrives with nothing running (an old notification).
        if (_state.value == RecState.Idle) stopSelf()
        return START_NOT_STICKY
    }

    private fun free() = try { StatFs(filesDir.path).availableBytes } catch (e: Exception) { Long.MAX_VALUE }

    private fun begin() {
        val free = free()
        if (!Space.canStart(free)) {
            _error.value = R.string.no_space_to_start
            _errorArg.value = (free / Space.MB).toInt()
            stopSelf()
            return
        }
        val now = System.currentTimeMillis()
        val zone = ZoneId.systemDefault()
        val id = SessionDir.newId(now)
        val state = SessionState(id = id, startedAt = now, zone = zone.id)
        val title = FileNames.displayTitle(Instant.ofEpochMilli(now).atZone(zone), null)
        _state.value = RecState.Recording(id, title, false, now, SystemClock.elapsedRealtime(), emptyList())
        activeId = id
        // The notification must be up before the microphone opens.
        try {
            startForeground(Notifications.ID_RECORDING, buildNotification(), ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE)
        } catch (e: Exception) {
            fail(R.string.error_cannot_record)
            return
        }
        try {
            sessions.write(state)
            recorder = MediaRecorder(this).apply {
                setAudioSource(MediaRecorder.AudioSource.MIC)
                setOutputFormat(MediaRecorder.OutputFormat.AAC_ADTS)
                setAudioEncoder(MediaRecorder.AudioEncoder.AAC)
                setAudioChannels(1)
                setAudioSamplingRate(SAMPLE_RATE)
                setAudioEncodingBitRate(BIT_RATE)
                setOutputFile(sessions.audio(id))
                setOnErrorListener { _, _, _ -> stopBecause(R.string.stopped_mic_error) }
                setOnInfoListener { _, what, _ ->
                    if (what == MediaRecorder.MEDIA_RECORDER_INFO_MAX_FILESIZE_REACHED ||
                        what == MediaRecorder.MEDIA_RECORDER_INFO_MAX_DURATION_REACHED
                    ) stopBecause(R.string.stopped_no_space)
                }
                prepare()
                start()
            }
        } catch (e: Exception) {
            recorder?.release()
            recorder = null
            sessions.clear(id)
            fail(R.string.error_mic_busy)
            return
        }
        wakeLock = getSystemService(PowerManager::class.java)
            .newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "seogi:recording")
            .apply { acquire(MAX_WAKE_MS) }
        // The microphone is open first; the calendar title follows, so a slow
        // calendar never cuts the start of a meeting.
        lifecycleScope.launch {
            val event = withContext(Dispatchers.IO) { CalendarPick.current(CalendarReader.today(this@RecorderService, now), now) }
                ?: return@launch
            val cur = _state.value as? RecState.Recording ?: return@launch
            if (cur.id != id) return@launch
            _state.value = cur.copy(title = FileNames.displayTitle(Instant.ofEpochMilli(now).atZone(zone), event.title), fromCalendar = true)
            sessions.read(id)?.let { sessions.write(it.copy(eventTitle = event.title)) }
            showNotification()
        }
        ticker = lifecycleScope.launch {
            var n = 0
            while (isActive) {
                val amp = try { recorder?.maxAmplitude ?: 0 } catch (e: Exception) { 0 }
                _level.value = (amp / 32767f).coerceIn(0f, 1f)
                // Every 30 s: is there still room to keep going and to save?
                if (++n % 250 == 0 && Space.mustStop(free(), sessions.audio(id).length())) {
                    stopBecause(R.string.stopped_no_space)
                    break
                }
                delay(120)
            }
        }
    }

    private fun mark() {
        val s = _state.value as? RecState.Recording ?: return
        val at = SystemClock.elapsedRealtime() - s.startedElapsed
        val next = s.copy(marksMs = s.marksMs + at)
        _state.value = next
        sessions.read(s.id)?.let { sessions.write(it.copy(marksMs = next.marksMs)) }
        showNotification()
    }

    private fun finish() {
        val s = _state.value as? RecState.Recording ?: return
        ticker?.cancel()
        try { recorder?.stop() } catch (e: Exception) { /* too short: the stream may be empty */ }
        recorder?.release()
        recorder = null
        wakeLock?.takeIf { it.isHeld }?.release()
        _state.value = RecState.Saving
        // Saved in the app's scope, not the service's, so the screens never
        // stay on "저장 중" even if the service goes away meanwhile.
        val app = application as SeogiApp
        app.scope.launch {
            try {
                app.saveSession(sessions, s.id, recovered = false)
            } finally {
                activeId = null
                _state.value = RecState.Idle
                withContext(Dispatchers.Main + NonCancellable) {
                    stopForeground(STOP_FOREGROUND_REMOVE)
                    stopSelf()
                }
            }
        }
    }

    /** Stopped by the phone, not the user: save what there is and say why. */
    private fun stopBecause(why: Int) {
        if (_state.value !is RecState.Recording) return
        Notifications.recordingStopped(this, why)
        finish()
    }

    /** "버리기": stop without saving; the stream and its notes are removed. */
    private fun discard() {
        val s = _state.value as? RecState.Recording ?: return
        ticker?.cancel()
        try { recorder?.stop() } catch (e: Exception) {}
        recorder?.release()
        recorder = null
        wakeLock?.takeIf { it.isHeld }?.release()
        sessions.clear(s.id)
        activeId = null
        _state.value = RecState.Idle
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
    }

    private fun fail(message: Int) {
        activeId = null
        _state.value = RecState.Idle
        _error.value = message
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
    }

    override fun onDestroy() {
        // Killed politely while recording: close the stream; the next start finishes it.
        if (recorder != null) {
            try { recorder?.stop() } catch (_: Exception) {}
            recorder?.release()
            recorder = null
            if (_state.value is RecState.Recording) {
                activeId = null
                _state.value = RecState.Idle
                // Only the service went away, the app lives on: save what there is now.
                (application as SeogiApp).recoverLeftovers()
            }
        }
        wakeLock?.takeIf { it.isHeld }?.release()
        super.onDestroy()
    }

    private fun showNotification() {
        if (_state.value !is RecState.Recording) return
        try {
            getSystemService(android.app.NotificationManager::class.java).notify(Notifications.ID_RECORDING, buildNotification())
        } catch (_: SecurityException) {}
    }

    private fun buildNotification(): android.app.Notification {
        val s = _state.value as RecState.Recording
        val elapsed = SystemClock.elapsedRealtime() - s.startedElapsed
        val text = if (s.marksMs.isEmpty()) getString(R.string.notif_recording)
        else getString(R.string.notif_recording_marks, s.marksMs.size)
        return Notifications.recordingWithLockScreen(
            this, s.title, text, System.currentTimeMillis() - elapsed,
            serviceIntent(ACTION_MARK, 1), serviceIntent(ACTION_STOP, 2),
        )
    }

    private fun serviceIntent(action: String, code: Int) = PendingIntent.getService(
        this, code, Intent(this, RecorderService::class.java).setAction(action),
        PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
    )

    companion object {
        const val ACTION_START = "io.github.lokimdj.seogi.START"
        const val ACTION_MARK = "io.github.lokimdj.seogi.MARK"
        const val ACTION_STOP = "io.github.lokimdj.seogi.STOP"
        const val ACTION_DISCARD = "io.github.lokimdj.seogi.DISCARD"
        /** AAC-LC mono for speech: 48 kHz, 64 kbps (about 29 MB an hour). */
        const val SAMPLE_RATE = 48_000
        const val BIT_RATE = 64_000
        private const val MAX_WAKE_MS = 6 * 60 * 60 * 1000L

        private val _state = MutableStateFlow<RecState>(RecState.Idle)
        val state: StateFlow<RecState> = _state.asStateFlow()

        private val _level = MutableStateFlow(0f)
        /** The microphone's loudness now, 0..1, for the level bars. */
        val level: StateFlow<Float> = _level.asStateFlow()

        private val _errorArg = MutableStateFlow<Int?>(null)
        /** A number for [error]'s text, such as the megabytes left. */
        val errorArg: StateFlow<Int?> = _errorArg.asStateFlow()

        private val _error = MutableStateFlow<Int?>(null)
        /** A string resource to show once, when a recording could not start. */
        val error: StateFlow<Int?> = _error.asStateFlow()
        fun clearError() {
            _error.value = null
            _errorArg.value = null
        }

        /** The id of the recording running now, which recovery must leave alone. */
        @Volatile var activeId: String? = null
            private set

        fun sessionDir(context: Context) = java.io.File(context.filesDir, "recording")

        fun start(context: Context) = ContextCompat.startForegroundService(
            context, Intent(context, RecorderService::class.java).setAction(ACTION_START),
        )

        fun mark(context: Context) = context.startService(Intent(context, RecorderService::class.java).setAction(ACTION_MARK))

        fun discard(context: Context) = context.startService(Intent(context, RecorderService::class.java).setAction(ACTION_DISCARD))

        fun stop(context: Context) = context.startService(Intent(context, RecorderService::class.java).setAction(ACTION_STOP))
    }
}
