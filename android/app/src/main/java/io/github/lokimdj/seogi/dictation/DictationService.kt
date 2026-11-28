package io.github.lokimdj.seogi.dictation

import android.Manifest
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.ClipData
import android.content.ClipboardManager
import android.content.ComponentCallbacks2
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.content.pm.ServiceInfo
import android.content.res.Configuration
import android.graphics.PixelFormat
import android.media.AudioManager
import android.media.AudioRecordingConfiguration
import android.os.Binder
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.os.SystemClock
import android.provider.Settings
import android.util.Log
import android.view.Gravity
import android.view.HapticFeedbackConstants
import android.view.MotionEvent
import android.view.WindowManager
import android.widget.Toast
import androidx.core.app.NotificationCompat
import io.github.lokimdj.seogi.R
import io.github.lokimdj.seogi.record.Notifications
import io.github.lokimdj.seogi.record.RecorderService
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.util.concurrent.Executors

/**
 * The floating key: a round microphone over other apps. Touch it and speak;
 * the words become text on the phone (Qwen3-ASR through transcribe.cpp) and go
 * into the field with the cursor, through [InsertService], or to the clipboard.
 * With the accessibility service on, the key comes up only while the user
 * types (a field has the cursor or the keyboard is up) and goes shortly after;
 * without it there is no way to know, so it stays up ([KeyRule]).
 *
 * Runs in its own process (`:dictation`): the speech engine is native code
 * holding 1.4 GB, and if it ever crashes or is killed for memory, a meeting
 * being recorded by [io.github.lokimdj.seogi.record.RecorderService] goes on.
 *
 * How it stays alive ([KeyKeeper]): with the accessibility service on,
 * [InsertService] holds it bound and the key is that service's overlay, so
 * nothing shows in the notifications while the key waits; it becomes a
 * microphone foreground service only while the microphone is open (Android
 * lets only those listen while another app is in front), started while the
 * key is on screen. Without accessibility nothing else keeps it, so it stays
 * a microphone foreground service with its notification.
 */
class DictationService : Service() {
    private val main = Handler(Looper.getMainLooper())
    private val ui = CoroutineScope(SupervisorJob() + Dispatchers.Main)
    private val engineThread = Executors.newSingleThreadExecutor { Thread(it, "seogi-asr") }
    private val engineContext = engineThread.asCoroutineDispatcher()
    private val engineScope = CoroutineScope(SupervisorJob() + engineContext)
    private val engine = Loaded(engineScope, IDLE_MS) { SpeechEngine.open(DictationModel.file(this)) }

    private val wm by lazy { getSystemService(WindowManager::class.java) }
    private var view: BubbleView? = null
    /** The window manager the key was added with (the accessibility service's own, for its overlay). */
    private var keyWm: WindowManager? = null
    /** Which way the key on screen was put up, so a change of way puts it up again. */
    private var viewWay: KeyKeeper.Way? = null
    private var params: WindowManager.LayoutParams? = null
    private val gesture by lazy { BubbleGesture(slop = 10 * resources.displayMetrics.density) }
    private var dragFromX = 0
    private var dragFromY = 0
    private var mode = BubbleGesture.Mode.IDLE
    private var mic: Mic? = null
    private var hidden = false
    private var foreground = false
    /** [InsertService] holds this service bound: the accessibility way ([KeyKeeper]). */
    private var bound = false
    /** The user left the key on, as last read from [DictationPrefs]. */
    private var on = false
    private val way get() = KeyKeeper.way(bound)
    private val presence = KeyPresence()
    private val settle = Runnable { refresh() }
    private var followsShown: Boolean? = null
    private var destroyed = false
    private val recordings = object : AudioManager.AudioRecordingCallback() {
        override fun onRecordingConfigChanged(configs: MutableList<AudioRecordingConfiguration>?) = refresh()
    }

    override fun onCreate() {
        super.onCreate()
        on = DictationPrefs.isOn(this)
        InsertService.listener = { refresh() }
        InsertService.keystroke = { typedByHand() }
        // A meeting starting or ending changes whether the key may show.
        getSystemService(AudioManager::class.java).registerAudioRecordingCallback(recordings, main)
    }

    // --- staying alive ---

    /** Bound by [InsertService] while accessibility is on: Android keeps the key, no notification needed. */
    override fun onBind(intent: Intent?): IBinder {
        kept()
        return Binder()
    }

    override fun onRebind(intent: Intent?) = kept()

    override fun onUnbind(intent: Intent?): Boolean {
        bound = false
        // Accessibility went off. Started as a foreground service as well, the
        // key goes on that way, with that way's notification (hide and off);
        // otherwise Android ends this service now and opening Seogi brings the
        // key back, with its notification.
        refresh(atOnce = true)
        if (foreground) updateNotification()
        return true
    }

    private fun kept() {
        bound = true
        on = DictationPrefs.isOn(this)
        // Hiding is done from the notification, which this way has none of.
        hidden = false
        settleForeground()
        refresh(atOnce = true)
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_OFF -> {
                DictationPrefs.setOn(this, false)
                on = false
                // Bound by accessibility, the service lives on without the key.
                refresh(atOnce = true)
                leaveForeground()
                stopSelf()
                return START_NOT_STICKY
            }
            ACTION_HIDE -> hidden = true
            ACTION_SHOW -> hidden = false
        }
        // Started with startForegroundService, it must say so within seconds,
        // even when it leaves the foreground again at once.
        val mustAnswer = intent?.getBooleanExtra(EXTRA_FOREGROUND, false) == true
        if ((mustAnswer || way == KeyKeeper.Way.FOREGROUND) && !goForeground()) {
            stopSelf()
            return START_NOT_STICKY
        }
        on = DictationPrefs.isOn(this)
        if (!on) {
            refresh(atOnce = true)
            leaveForeground()
            stopSelf()
            return START_NOT_STICKY
        }
        settleForeground()
        refresh(atOnce = true)
        if (foreground) updateNotification()
        if (!foreground) stopSelf(startId)
        return if (way == KeyKeeper.Way.FOREGROUND) START_STICKY else START_NOT_STICKY
    }

    /**
     * Leaves the foreground, and so drops the notification, when nothing
     * needs it any more ([KeyKeeper.foreground]): the accessibility way with
     * the microphone closed. The binding keeps the service.
     */
    private fun settleForeground() {
        if (!foreground || KeyKeeper.foreground(way, mic != null)) return
        leaveForeground()
        stopSelf()
    }

    private fun leaveForeground() {
        if (!foreground) return
        stopForeground(STOP_FOREGROUND_REMOVE)
        foreground = false
    }

    /**
     * The accessibility way, before the microphone opens: Android lets a
     * microphone foreground service start from behind another app while
     * Seogi has a window on screen, which the key the user is touching is.
     * False when Android refused all the same.
     */
    private fun holdMic(): Boolean {
        if (foreground) return true
        try {
            startForegroundService(Intent(this, DictationService::class.java).putExtra(EXTRA_FOREGROUND, true))
        } catch (e: Exception) {
            Log.w(TAG, "could not start listening in the foreground", e)
            return false
        }
        return goForeground()
    }

    /**
     * Puts the key on the screen or takes it off, by [KeyRule], after the
     * wish has held a moment ([KeyPresence]) unless [atOnce].
     */
    private fun refresh(atOnce: Boolean = false) {
        main.removeCallbacks(settle)
        if (destroyed) return
        val follows = InsertService.instance != null
        val meeting by lazy {
            MeetingCheck.recording(
                RecorderService.sessionDir(this), System.currentTimeMillis(),
                getSystemService(AudioManager::class.java).activeRecordingConfigurations.isNotEmpty(),
            )
        }
        val alive = KeyKeeper.alive(way, foreground, on)
        val want = alive && KeyRule.wanted(mode, hidden, meeting, follows, InsertService.typing)
        val wait = presence.update(want, SystemClock.uptimeMillis(), atOnce || !alive)
        if (wait != null && wait > 0) main.postDelayed(settle, wait)
        // Put up again when the way changed: the accessibility overlay belongs to that service.
        if (view != null && viewWay != way) removeBubble()
        if (presence.shown) showBubble() else removeBubble()
        // The keyboard came up, went or changed height: keep the key off it.
        if (view != null && floor() != placedFloor) placeAt(DictationPrefs.place(this))
        if (follows != followsShown) {
            val first = followsShown == null
            followsShown = follows
            if (!first && foreground) updateNotification()
        }
    }

    /** Into the foreground, with the notification of the way the key is kept. */
    private fun goForeground(): Boolean {
        if (foreground) return true
        if (checkSelfPermission(Manifest.permission.RECORD_AUDIO) != PackageManager.PERMISSION_GRANTED) return false
        return try {
            startForeground(ID_NOTIFICATION, notification(), ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE)
            foreground = true
            true
        } catch (e: Exception) {
            // Android refused a microphone service started from the background
            // (a restart after the process was killed). Opening Seogi starts it again.
            Log.w(TAG, "could not start in the foreground", e)
            false
        }
    }

    // --- the key on the screen ---

    private fun showBubble() {
        if (view != null) return
        // The accessibility way: the accessibility service's own overlay, which
        // needs neither the overlay permission nor a notification.
        val insert = if (way == KeyKeeper.Way.ACCESSIBILITY) InsertService.instance ?: return else null
        if (insert == null && !Settings.canDrawOverlays(this)) return
        val host = insert?.getSystemService(WindowManager::class.java) ?: wm
        val size = fullSize()
        val v = BubbleView(this, ::onTouch, ::fit)
        v.mode = mode
        v.setAccessibilityDelegate(object : android.view.View.AccessibilityDelegate() {
            override fun performAccessibilityAction(host: android.view.View, action: Int, args: android.os.Bundle?): Boolean {
                if (action == android.view.accessibility.AccessibilityNodeInfo.ACTION_CLICK) {
                    tap()
                    return true
                }
                return super.performAccessibilityAction(host, action, args)
            }

            override fun onInitializeAccessibilityNodeInfo(host: android.view.View, info: android.view.accessibility.AccessibilityNodeInfo) {
                super.onInitializeAccessibilityNodeInfo(host, info)
                info.isClickable = true
                info.addAction(android.view.accessibility.AccessibilityNodeInfo.AccessibilityAction.ACTION_CLICK)
            }
        })
        val p = WindowManager.LayoutParams(
            size, size,
            if (insert != null) WindowManager.LayoutParams.TYPE_ACCESSIBILITY_OVERLAY else WindowManager.LayoutParams.TYPE_APPLICATION_OVERLAY,
            // Never takes the keyboard or the focus, so the field in the app behind keeps its cursor.
            WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE or WindowManager.LayoutParams.FLAG_LAYOUT_IN_SCREEN,
            PixelFormat.TRANSLUCENT,
        ).apply { gravity = Gravity.TOP or Gravity.START }
        params = p
        viewWay = way
        placeAt(DictationPrefs.place(this))
        try {
            host.addView(v, p)
            view = v
            keyWm = host
            if (steppedBack) v.stepBack(true, animate = false)
        } catch (e: Exception) {
            Log.w(TAG, "could not show the key", e)
            params = null
            viewWay = null
        }
    }

    private fun removeBubble() {
        val v = view ?: return
        val host = keyWm ?: wm
        view = null
        params = null
        keyWm = null
        viewWay = null
        if (mode == BubbleGesture.Mode.LISTENING) cancelListening()
        try { host.removeView(v) } catch (_: Exception) {}
    }

    private fun screen(): Pair<Int, Int> {
        val b = wm.currentWindowMetrics.bounds
        return Pair(b.width(), b.height())
    }

    /**
     * Room kept free at the left and right: the back gesture's strip, and at
     * least [EDGE_DP], so the key is never under the thumb's swipe back.
     */
    private fun sideMargin(): Int {
        val least = (EDGE_DP * resources.displayMetrics.density).toInt()
        val g = try {
            wm.currentWindowMetrics.windowInsets.getInsets(android.view.WindowInsets.Type.systemGestures())
        } catch (_: Exception) { null }
        return maxOf(least, g?.left ?: 0, g?.right ?: 0)
    }

    /**
     * The keyboard's top edge, when the key is the accessibility overlay: that
     * sits above the keyboard, so it is kept out of the keyboard's way.
     */
    private fun floor(): Int? = if (viewWay == KeyKeeper.Way.ACCESSIBILITY) InsertService.keyboardTop else null
    private var placedFloor: Int? = null

    private fun placeAt(place: BubblePlace) {
        val p = params ?: return
        val (w, h) = screen()
        placedFloor = floor()
        // Placed as the full key; a key stepped back (a smaller window) keeps the same centre.
        val full = fullSize()
        val (x, y) = place.toPixels(w, h, full, sideMargin(), placedFloor)
        p.x = x + (full - p.width) / 2
        p.y = y + (full - p.height) / 2
        view?.let { try { (keyWm ?: wm).updateViewLayout(it, p) } catch (_: Exception) {} }
    }

    private fun fullSize() = (SIZE_DP * resources.displayMetrics.density).toInt()

    /**
     * The window fits the face: smaller while the key steps back, so a touch
     * beside the small face reaches the keyboard or app below.
     */
    private fun fit(small: Boolean) {
        val p = params ?: return
        val n = if (small) (fullSize() * KeyFace.BACK).toInt() else fullSize()
        if (p.width == n) return
        p.width = n
        p.height = n
        placeAt(DictationPrefs.place(this))
    }

    override fun onConfigurationChanged(newConfig: Configuration) {
        super.onConfigurationChanged(newConfig)
        // The Fold opened or closed, or the phone turned: same side, same height.
        placeAt(DictationPrefs.place(this))
    }

    /**
     * Once the user types by hand the key steps back (small and faint) and
     * stays so, also when it goes and comes back with the keyboard, until the
     * user touches it: it should not pop up big in the middle of typing.
     */
    private var steppedBack = false

    private fun typedByHand() {
        if (mode != BubbleGesture.Mode.IDLE) return
        steppedBack = true
        view?.stepBack(true)
    }

    /** A touch that began off the face (in the window's margin) is let go whole. */
    private var offFace = false
    /** The screen and side margin, read once when a touch begins and used while it drags. */
    private var dragScreen = Pair(0, 0)
    private var dragMargin = 0

    private fun onTouch(e: MotionEvent) {
        if (e.actionMasked == MotionEvent.ACTION_DOWN) {
            offFace = view?.onFace(e.x, e.y) == false
        }
        if (offFace) return
        when (e.actionMasked) {
            MotionEvent.ACTION_DOWN -> {
                steppedBack = false
                dragScreen = screen()
                dragMargin = sideMargin()
                view?.press()
            }
            MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL -> view?.release()
        }
        val acts = when (e.actionMasked) {
            MotionEvent.ACTION_DOWN -> {
                params?.let { dragFromX = it.x; dragFromY = it.y }
                gesture.down(e.eventTime, e.rawX, e.rawY, mode)
            }
            MotionEvent.ACTION_MOVE -> gesture.move(e.rawX, e.rawY)
            MotionEvent.ACTION_UP -> gesture.up(e.eventTime)
            MotionEvent.ACTION_CANCEL -> gesture.cancel()
            else -> emptyList()
        }
        acts.forEach(::act)
    }

    private fun act(a: BubbleGesture.Act) {
        when (a) {
            BubbleGesture.Act.Start -> startListening()
            BubbleGesture.Act.Stop -> stopAndWrite()
            BubbleGesture.Act.Cancel -> cancelListening()
            is BubbleGesture.Act.Move -> {
                val p = params ?: return
                val (w, h) = dragScreen
                val m = dragMargin.coerceIn(0, ((w - p.width) / 2).coerceAtLeast(0))
                p.x = (dragFromX + a.dx.toInt()).coerceIn(m, (w - p.width - m).coerceAtLeast(m))
                p.y = BubblePlace.above((dragFromY + a.dy.toInt()).coerceIn(0, (h - p.height).coerceAtLeast(0)), p.height, floor())
                view?.let { try { (keyWm ?: wm).updateViewLayout(it, p) } catch (_: Exception) {} }
            }
            BubbleGesture.Act.DragEnd -> {
                val p = params ?: return
                val (w, h) = dragScreen
                val place = BubblePlace.afterDrag(p.x, p.y, w, h, p.width, dragMargin)
                DictationPrefs.setPlace(this, place)
                placeAt(place)
            }
        }
    }

    /** A tap from TalkBack: start, or stop and write. */
    private fun tap() {
        when (mode) {
            BubbleGesture.Mode.IDLE -> startListening()
            BubbleGesture.Mode.LISTENING -> stopAndWrite()
            BubbleGesture.Mode.WORKING -> {}
        }
    }

    private fun setMode(m: BubbleGesture.Mode) {
        mode = m
        view?.mode = m
        // Listening or writing keeps the key up; back to idle, it follows the typing again.
        refresh(atOnce = m != BubbleGesture.Mode.IDLE)
    }

    // --- listening and writing ---

    private fun startListening() {
        if (mode != BubbleGesture.Mode.IDLE) return
        val why = when {
            checkSelfPermission(Manifest.permission.RECORD_AUDIO) != PackageManager.PERMISSION_GRANTED -> R.string.dict_err_mic_permission
            !DictationModel.isReady(this) -> R.string.dict_err_no_model
            !SpeechEngine.available() -> R.string.dict_err_cpu
            MeetingCheck.recording(
                RecorderService.sessionDir(this), System.currentTimeMillis(),
                getSystemService(AudioManager::class.java).activeRecordingConfigurations.isNotEmpty(),
            ) -> R.string.dict_err_meeting
            else -> null
        }
        if (why != null) {
            toast(getString(why))
            return
        }
        if (KeyKeeper.foreground(way, micOpen = true) && !holdMic()) {
            toast(getString(R.string.dict_err_mic_silenced))
            return
        }
        val m = Mic(onFull = { main.post { if (mode == BubbleGesture.Mode.LISTENING) stopAndWrite() } })
        if (!m.start()) {
            settleForeground()
            toast(getString(R.string.dict_err_mic_busy))
            return
        }
        mic = m
        setMode(BubbleGesture.Mode.LISTENING)
        view?.performHapticFeedback(HapticFeedbackConstants.VIRTUAL_KEY)
        // Load the model while the user is still speaking, once the touch is
        // not a drag (a drag cancels this before it runs).
        main.postDelayed(warm, WARM_AFTER_MS)
    }

    private val warm = Runnable {
        engineScope.launch {
            try { engine.warm() } catch (e: Throwable) { Log.w(TAG, "could not load the model", e) }
        }
    }

    private fun cancelListening() {
        main.removeCallbacks(warm)
        mic?.cancel()
        mic = null
        settleForeground()
        if (mode == BubbleGesture.Mode.LISTENING) setMode(BubbleGesture.Mode.IDLE)
    }

    private fun stopAndWrite() {
        val m = mic ?: return
        mic = null
        val pcm = m.stop()
        // The microphone is closed: the accessibility way leaves the foreground now, before the writing.
        settleForeground()
        view?.performHapticFeedback(HapticFeedbackConstants.VIRTUAL_KEY)
        if (pcm.size < Speech.MIN_SAMPLES) {
            setMode(BubbleGesture.Mode.IDLE)
            return
        }
        if (pcm.all { it == 0f }) {
            // Android gave us silence: the microphone is blocked for this service until Seogi is opened.
            setMode(BubbleGesture.Mode.IDLE)
            toast(getString(R.string.dict_err_mic_silenced))
            return
        }
        setMode(BubbleGesture.Mode.WORKING)
        ui.launch {
            val text = try {
                withContext(engineContext) {
                    engine.use { e -> Speech.written(Speech.pieces(pcm).map { e.transcribe(it) }) }
                }
            } catch (e: CancellationException) {
                throw e // the key was turned off meanwhile: nothing failed, nothing to say
            } catch (e: Throwable) {
                Log.w(TAG, "transcription failed", e)
                null
            }
            setMode(BubbleGesture.Mode.IDLE)
            when {
                text == null -> toast(getString(R.string.dict_err_failed))
                text.isEmpty() -> toast(getString(R.string.dict_heard_nothing))
                else -> deliver(text)
            }
        }
    }

    /** Into the field with the cursor; when there is none (or no accessibility), onto the clipboard. */
    private fun deliver(text: String) {
        val service = InsertService.instance
        if (service == null) Log.i(TAG, "copying instead of typing: accessibility is off")
        val typed = try {
            service?.insert(text) == true
        } catch (e: Exception) {
            Log.i(TAG, "copying instead of typing: the insert failed", e)
            false
        }
        if (typed) return
        getSystemService(ClipboardManager::class.java)?.setPrimaryClip(ClipData.newPlainText(getString(R.string.app_name), text))
        toast(getString(R.string.dict_copied))
    }

    private fun toast(text: String) = Toast.makeText(this, text, Toast.LENGTH_SHORT).show()

    // --- the notification ---

    /**
     * The notification Android asks of a microphone foreground service. The
     * accessibility way shows one only while listening, without buttons:
     * Android then holds it back for its first ten seconds, so a short
     * sentence shows only the microphone dot.
     */
    private fun notification(): Notification {
        val nm = getSystemService(NotificationManager::class.java)
        nm.createNotificationChannel(
            NotificationChannel(CHANNEL, getString(R.string.channel_dictation), NotificationManager.IMPORTANCE_LOW).apply {
                setShowBadge(false)
            },
        )
        if (way == KeyKeeper.Way.ACCESSIBILITY) {
            return NotificationCompat.Builder(this, CHANNEL)
                .setSmallIcon(R.drawable.ic_dictation)
                .setContentTitle(getString(R.string.notif_dictation_listening))
                .setOngoing(true)
                .setSilent(true)
                .setCategory(NotificationCompat.CATEGORY_SERVICE)
                .setForegroundServiceBehavior(NotificationCompat.FOREGROUND_SERVICE_DEFAULT)
                .setContentIntent(Notifications.openApp(this))
                .build()
        }
        fun action(a: String) = PendingIntent.getService(
            this, a.hashCode(), Intent(this, DictationService::class.java).setAction(a),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        return NotificationCompat.Builder(this, CHANNEL)
            .setSmallIcon(R.drawable.ic_dictation)
            .setContentTitle(getString(R.string.notif_dictation))
            .setContentText(
                getString(
                    when {
                        hidden -> R.string.notif_dictation_hidden
                        InsertService.instance != null -> R.string.notif_dictation_typing
                        else -> R.string.notif_dictation_text
                    },
                ),
            )
            .setOngoing(true)
            .setSilent(true)
            .setCategory(NotificationCompat.CATEGORY_SERVICE)
            .setContentIntent(Notifications.openApp(this))
            .addAction(
                0, getString(if (hidden) R.string.action_show_key else R.string.action_hide_key),
                action(if (hidden) ACTION_SHOW else ACTION_HIDE),
            )
            .addAction(0, getString(R.string.action_dictation_off), action(ACTION_OFF))
            .build()
    }

    private fun updateNotification() {
        try {
            getSystemService(NotificationManager::class.java).notify(ID_NOTIFICATION, notification())
        } catch (_: SecurityException) {}
    }

    override fun onTrimMemory(level: Int) {
        super.onTrimMemory(level)
        // The phone is short of memory: let the model go while nobody is speaking.
        if (level >= ComponentCallbacks2.TRIM_MEMORY_RUNNING_LOW && mode == BubbleGesture.Mode.IDLE) {
            engineScope.launch { engine.close() }
        }
    }

    override fun onDestroy() {
        destroyed = true
        main.removeCallbacks(warm)
        main.removeCallbacks(settle)
        InsertService.listener = null
        InsertService.keystroke = null
        try { getSystemService(AudioManager::class.java).unregisterAudioRecordingCallback(recordings) } catch (_: Exception) {}
        mic?.cancel()
        mic = null
        removeBubble()
        ui.cancel()
        engineScope.launch {
            engine.close()
            engineThread.shutdown()
        }
        super.onDestroy()
    }

    companion object {
        private const val TAG = "SeogiDictation"
        private const val CHANNEL = "dictation"
        private const val ID_NOTIFICATION = 11
        /** The full key's window; the face fills [KeyFace.FACE] of it (about 55 dp across). */
        private const val SIZE_DP = 59
        /** The least room at each side, wider than the back gesture strips of common phones. */
        private const val EDGE_DP = 28
        private const val WARM_AFTER_MS = 250L
        /** The model is let go after this long without speaking. */
        const val IDLE_MS = 3 * 60_000L
        private const val ACTION_HIDE = "io.github.lokimdj.seogi.dictation.HIDE"
        private const val ACTION_SHOW = "io.github.lokimdj.seogi.dictation.SHOW"
        private const val ACTION_OFF = "io.github.lokimdj.seogi.dictation.OFF"
        /** Set on an intent sent with startForegroundService: the service must go into the foreground. */
        internal const val EXTRA_FOREGROUND = "io.github.lokimdj.seogi.dictation.FOREGROUND"

        /**
         * Turns the key on, or brings it back if Android stopped it. Call while
         * Seogi is on screen. With accessibility on, [InsertService] keeps the
         * service and a plain start is enough; otherwise it is a foreground service.
         */
        fun start(context: Context) {
            DictationPrefs.setOn(context, true)
            val intent = Intent(context, DictationService::class.java)
            try {
                if (Accessibility.isOn(context)) {
                    context.startService(intent)
                } else {
                    context.startForegroundService(intent.putExtra(EXTRA_FOREGROUND, true))
                }
            } catch (e: Exception) {
                Log.w(TAG, "could not start", e)
            }
        }

        /**
         * Brings the key back without Seogi on screen (the app was updated, the
         * phone started), when the user left it on and it has what it needs.
         * With accessibility on there is nothing to do: Android binds
         * [InsertService] again, and that brings the key. Otherwise Android may
         * still refuse a microphone service started from the background; that
         * is logged, and opening Seogi starts it.
         */
        fun resume(context: Context, why: String) {
            if (!DictationPrefs.isOn(context)) return
            if (Accessibility.isOn(context)) {
                Log.i(TAG, "the key comes back with accessibility ($why)")
                return
            }
            val ready = context.checkSelfPermission(Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED &&
                Settings.canDrawOverlays(context) && DictationModel.isReady(context)
            if (!ready) {
                Log.i(TAG, "not bringing the key back ($why): permission, overlay or model missing")
                return
            }
            try {
                context.startForegroundService(Intent(context, DictationService::class.java).putExtra(EXTRA_FOREGROUND, true))
                Log.i(TAG, "bringing the key back ($why)")
            } catch (e: Exception) {
                Log.i(TAG, "Android did not let the key come back ($why)", e)
            }
        }

        /** Turns the key off. Call while Seogi is on screen. */
        fun stop(context: Context) {
            DictationPrefs.setOn(context, false)
            // Bound by accessibility, stopService would not reach it: it is told instead.
            try {
                context.startService(Intent(context, DictationService::class.java).setAction(ACTION_OFF))
            } catch (e: Exception) {
                Log.w(TAG, "could not tell the key to go", e)
                context.stopService(Intent(context, DictationService::class.java))
            }
        }
    }
}
