package io.github.lokimdj.seogi

import android.Manifest
import android.accounts.AccountManager
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Bundle
import android.os.PowerManager
import android.os.StatFs
import android.provider.Settings as SystemSettings
import android.widget.Toast
import androidx.activity.ComponentActivity
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.IntentSenderRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LifecycleEventEffect
import androidx.lifecycle.lifecycleScope
import androidx.work.WorkManager
import com.google.android.gms.auth.api.identity.AuthorizationResult
import com.google.android.gms.common.api.ApiException
import com.google.android.gms.common.api.CommonStatusCodes
import io.github.lokimdj.seogi.data.ListEntry
import io.github.lokimdj.seogi.data.MeetingLibrary
import io.github.lokimdj.seogi.data.MeetingList
import io.github.lokimdj.seogi.data.Recording
import io.github.lokimdj.seogi.data.UploadError
import io.github.lokimdj.seogi.data.UploadState
import io.github.lokimdj.seogi.record.CalendarEvent
import io.github.lokimdj.seogi.record.CalendarPick
import io.github.lokimdj.seogi.record.CalendarReader
import io.github.lokimdj.seogi.record.RecState
import io.github.lokimdj.seogi.record.RecorderService
import io.github.lokimdj.seogi.record.Space
import io.github.lokimdj.seogi.ui.AudioPlayer
import io.github.lokimdj.seogi.ui.AudioState
import io.github.lokimdj.seogi.ui.ConfirmDialog
import io.github.lokimdj.seogi.ui.MeetingScreen
import io.github.lokimdj.seogi.ui.PlayState
import io.github.lokimdj.seogi.ui.DictationScreen
import io.github.lokimdj.seogi.ui.LicensesScreen
import io.github.lokimdj.seogi.ui.UndoBar
import io.github.lokimdj.seogi.ui.HomeScreen
import io.github.lokimdj.seogi.ui.OnboardingScreen
import io.github.lokimdj.seogi.ui.Perms
import io.github.lokimdj.seogi.ui.ReadingWidth
import io.github.lokimdj.seogi.ui.RecordingScreen
import io.github.lokimdj.seogi.ui.SeogiTheme
import io.github.lokimdj.seogi.ui.SettingsScreen
import io.github.lokimdj.seogi.ui.UploadsScreen
import io.github.lokimdj.seogi.ui.pal
import io.github.lokimdj.seogi.upload.DeleteWindow
import io.github.lokimdj.seogi.upload.Deleter
import io.github.lokimdj.seogi.upload.DriveApi
import io.github.lokimdj.seogi.upload.GoogleAuth
import io.github.lokimdj.seogi.upload.UploadQueue
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

private enum class Screen { ONBOARDING, HOME, RECORDING, UPLOADS, SETTINGS, DICTATION, LICENSES, MEETING }

class MainActivity : ComponentActivity() {
    private val app get() = application as SeogiApp
    private var pickedEmail: String? = null

    private var deleteTarget by mutableStateOf<Recording?>(null)

    /** Plays the open meeting's recording (read only). */
    private val player by lazy { AudioPlayer(this) }
    private var audioState by mutableStateOf<AudioState>(AudioState.None)
    private var playState by mutableStateOf(PlayState(false, 0, 0))
    private var audioUri: Uri? = null
    private var openKey: String? = null

    /** Why the last sign-in did not work, shown in red until the next try. */
    private var loginMessage by mutableStateOf<String?>(null)

    private val consent = registerForActivityResult(ActivityResultContracts.StartIntentSenderForResult()) { res ->
        if (res.resultCode != RESULT_OK) {
            // Back from Google's screen without a grant: cancelled, or Google
            // blocked this account (the consent screen is still in testing).
            loginFailed(getString(R.string.login_not_finished))
            return@registerForActivityResult
        }
        try {
            onAuthorized(GoogleAuth.resultFromIntent(this, res.data))
        } catch (e: Exception) {
            loginFailed(loginErrorText(e))
        }
    }

    /** Android's own account chooser, so "다른 계정" really offers the other accounts. */
    private val chooseAccount = registerForActivityResult(ActivityResultContracts.StartActivityForResult()) { res ->
        val name = res.data?.getStringExtra(AccountManager.KEY_ACCOUNT_NAME)
        if (res.resultCode == RESULT_OK && name != null) authorizeFor(name)
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        app.recoverLeftovers()
        setContent { SeogiTheme { Root() } }
    }

    /** Sign in for Drive (`drive.file`); the consent screen only when Google needs it. */
    private fun login() {
        lifecycleScope.launch { authorizeFor(GoogleAuth.pickAccount(this@MainActivity)) }
    }

    /**
     * "다른 계정": without asking for a specific account Google quietly hands
     * back the one already approved, so nothing seemed to happen. Ask Android
     * which account first, then authorise that one.
     */
    private fun switchAccount() {
        val intent = AccountManager.newChooseAccountIntent(null, null, arrayOf("com.google"), null, null, null, null)
        try {
            chooseAccount.launch(intent)
        } catch (e: Exception) {
            login()
        }
    }

    private fun authorizeFor(email: String?) {
        pickedEmail = email
        loginMessage = null
        lifecycleScope.launch {
            try {
                val r = GoogleAuth.authorize(this@MainActivity, email)
                val pending = r.pendingIntent
                if (r.hasResolution() && pending != null) {
                    consent.launch(IntentSenderRequest.Builder(pending.intentSender).build())
                } else {
                    onAuthorized(r)
                }
            } catch (e: Exception) {
                loginFailed(loginErrorText(e))
            }
        }
    }

    private fun loginFailed(text: String) {
        loginMessage = text
        Toast.makeText(this, text, Toast.LENGTH_LONG).show()
    }

    private fun loginErrorText(e: Exception): String {
        val code = (e as? ApiException)?.statusCode
        return when (code) {
            CommonStatusCodes.CANCELED -> getString(R.string.login_not_finished)
            CommonStatusCodes.NETWORK_ERROR -> getString(R.string.login_offline)
            CommonStatusCodes.DEVELOPER_ERROR -> getString(R.string.login_app_setup, code)
            null -> getString(R.string.login_failed)
            else -> getString(R.string.login_failed_code, getString(R.string.login_failed), code)
        }
    }

    private fun onAuthorized(r: AuthorizationResult) {
        val token = r.accessToken ?: return
        lifecycleScope.launch {
            val email = pickedEmail
                ?: r.toGoogleSignInAccount()?.email
                ?: withContext(Dispatchers.IO) { runCatching { DriveApi().email(token) }.getOrNull() }
            val before = app.settings.email.value
            app.settings.setEmail(email ?: "")
            loginMessage = null
            if (!email.isNullOrEmpty()) {
                Toast.makeText(this@MainActivity, getString(R.string.login_done, email), Toast.LENGTH_SHORT).show()
            }
            val switched = before != null && before != (email ?: "")
            if (switched) {
                // Another account: half-done uploads belong to the old Drive; start them over.
                app.store.all.value.filter { it.upload != UploadState.UPLOADED }.forEach { rec ->
                    app.store.update(rec.id) { it.copy(sessionUri = null, sidecarFileId = null, progress = 0f) }
                }
            }
            app.store.all.value.filter { it.error == UploadError.NEED_LOGIN }.forEach { rec ->
                app.store.update(rec.id) { it.copy(upload = UploadState.WAITING, error = null) }
            }
            UploadQueue.enqueue(this@MainActivity, if (switched) UploadQueue.Mode.REPLACE else UploadQueue.Mode.APPEND)
            app.refreshMeetings(now = true)
        }
    }

    /** Confirmed: hidden and kept from uploading; the real delete waits out the undo window. */
    private fun startDelete(r: Recording, trashInDrive: Boolean) {
        if (app.store.get(r.id)?.upload == UploadState.UPLOADING) {
            // Stop the upload under way; the queue comes back after the delete or the undo.
            WorkManager.getInstance(this).cancelUniqueWork(UploadQueue.NAME)
        }
        app.deletes.start(r, trashInDrive)
    }

    private fun retry(r: Recording) {
        lifecycleScope.launch {
            if (r.error == UploadError.NEED_LOGIN || app.settings.email.value == null) {
                login()
                return@launch
            }
            app.store.update(r.id) { it.copy(upload = UploadState.WAITING, error = null) }
            UploadQueue.enqueue(this@MainActivity, UploadQueue.Mode.APPEND)
        }
    }

    private fun setWifiOnly(v: Boolean) {
        app.settings.setWifiOnly(v)
        UploadQueue.enqueue(this, UploadQueue.Mode.REPLACE)
    }

    private fun open(url: String) {
        try {
            startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(url)))
        } catch (_: Exception) {}
    }

    private fun freeBytes() = try { StatFs(filesDir.path).availableBytes } catch (e: Exception) { Long.MAX_VALUE }

    private fun granted(p: String) = checkSelfPermission(p) == PackageManager.PERMISSION_GRANTED

    private fun perms() = Perms(
        mic = granted(Manifest.permission.RECORD_AUDIO),
        notifications = granted(Manifest.permission.POST_NOTIFICATIONS),
        calendar = granted(Manifest.permission.READ_CALENDAR),
    )

    private fun openAppSettings() = startActivity(
        Intent(SystemSettings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.fromParts("package", packageName, null)),
    )

    /** A meeting was opened: where its recording is, without downloading anything. */
    private fun openAudio(e: ListEntry) {
        if (openKey == e.key) return
        player.release()
        openKey = e.key
        playState = PlayState(false, 0, 0)
        val local = e.recording?.mediaUri?.let(Uri::parse)
        val m = e.meeting
        audioUri = local
        audioState = when {
            local != null -> AudioState.Ready
            m?.audioFileId != null -> AudioState.InDrive(m.audioSize)
            else -> AudioState.None
        }
    }

    private fun closeAudio() {
        player.release()
        openKey = null
        audioUri = null
        playState = PlayState(false, 0, 0)
    }

    /**
     * Plays from [ms] (or from where it was). A recording only in Drive is
     * downloaded first, now that the user asked for it (issue #67, truth 4).
     */
    private fun playAt(e: ListEntry, ms: Long?) {
        when (val st = audioState) {
            AudioState.Ready -> {
                val uri = audioUri ?: return
                if (!player.play(uri, ms)) {
                    val m = e.meeting
                    if (uri.scheme != "file" && m?.audioFileId != null) {
                        // The phone's own copy is gone: the one in Drive is the same recording.
                        audioUri = null
                        audioState = AudioState.InDrive(m.audioSize)
                        playAt(e, ms)
                        return
                    }
                    Toast.makeText(this, R.string.p_cannot_play, Toast.LENGTH_SHORT).show()
                    return
                }
                playState = PlayState(true, player.positionMs, player.durationMs)
            }
            is AudioState.InDrive, AudioState.Failed -> {
                val m = e.meeting ?: return
                audioState = AudioState.Downloading(0f)
                val key = e.key
                lifecycleScope.launch {
                    val file = try {
                        withContext(Dispatchers.IO) {
                            app.meetings.audio(m.id, cacheDir) { p ->
                                lifecycleScope.launch(Dispatchers.Main) {
                                    if (openKey == key && audioState is AudioState.Downloading) audioState = AudioState.Downloading(p)
                                }
                            }
                        }
                    } catch (x: MeetingLibrary.NeedLogin) {
                        login()
                        null
                    } catch (x: Exception) {
                        null
                    }
                    if (openKey != key) return@launch
                    if (file == null) {
                        audioState = AudioState.Failed
                    } else {
                        audioUri = Uri.fromFile(file)
                        audioState = AudioState.Ready
                        playAt(e, ms)
                    }
                }
            }
            is AudioState.Downloading, AudioState.None -> Unit
        }
    }

    override fun onStop() {
        super.onStop()
        if (player.playing) player.pause()
        playState = playState.copy(playing = false)
    }

    override fun onDestroy() {
        player.release()
        super.onDestroy()
    }

    @Composable
    private fun Root() {
        val settings = app.settings
        var screen by rememberSaveable { mutableStateOf(if (settings.onboarded) Screen.HOME else Screen.ONBOARDING) }
        var perms by remember { mutableStateOf(perms()) }
        var now by remember { mutableLongStateOf(System.currentTimeMillis()) }
        var events by remember { mutableStateOf(emptyList<CalendarEvent>()) }
        val rec by RecorderService.state.collectAsState()
        val all by app.store.all.collectAsState()
        val hidden by app.pendingDeletes.collectAsState()
        val recordings = all.filter { it.id !in hidden }
        val account by settings.email.collectAsState()
        val wifiOnly by settings.wifiOnly.collectAsState()
        val error by RecorderService.error.collectAsState()
        val saveProblem by app.saveProblem.collectAsState()
        val driveMeetings by app.meetings.all.collectAsState()
        val meetingsProblem by app.meetingsProblem.collectAsState()
        val entries = MeetingList.merge(recordings, driveMeetings)
        var openEntry by rememberSaveable { mutableStateOf<String?>(null) }
        var openedFrom by rememberSaveable { mutableStateOf(Screen.HOME) }

        fun refresh() {
            app.recoverLeftovers()
            app.refreshMeetings()
            perms = perms()
            now = System.currentTimeMillis()
            lifecycleScope.launch { events = withContext(Dispatchers.IO) { CalendarReader.today(this@MainActivity) } }
        }
        LifecycleEventEffect(Lifecycle.Event.ON_RESUME) { refresh() }
        // Leaving the app ends the undo window: the delete happens now.
        LifecycleEventEffect(Lifecycle.Event.ON_STOP) {
            val screenOn = getSystemService(PowerManager::class.java).isInteractive
            if (DeleteWindow.commitOnStop(isChangingConfigurations, screenOn)) app.deletes.commitNow()
        }
        val pending by app.deletes.pending.collectAsState()
        LaunchedEffect(Unit) {
            while (true) {
                kotlinx.coroutines.delay(30_000)
                now = System.currentTimeMillis()
            }
        }
        LaunchedEffect(error) {
            error?.let {
                val arg = RecorderService.errorArg.value
                Toast.makeText(this@MainActivity, if (arg != null) getString(it, arg) else getString(it), Toast.LENGTH_LONG).show()
                RecorderService.clearError()
                if (screen == Screen.RECORDING && rec is RecState.Idle) screen = Screen.HOME
            }
        }
        // Stopped from the notification, or saved: back home.
        LaunchedEffect(rec) { if (rec is RecState.Idle && screen == Screen.RECORDING) screen = Screen.HOME }

        // A permission asked twice and refused: Android no longer shows the
        // dialog, so the key opens the app's settings instead.
        var asked by rememberSaveable { mutableStateOf(setOf<String>()) }
        val ask = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { refresh() }
        fun request(p: String) {
            if (p in asked && !shouldShowRequestPermissionRationale(p)) openAppSettings() else ask.launch(p)
            asked = asked + p
        }
        val askMicThenRecord = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { ok ->
            refresh()
            if (ok && !Space.canStart(freeBytes())) {
                Toast.makeText(
                    this@MainActivity, getString(R.string.no_space_to_start, (freeBytes() / Space.MB).toInt()), Toast.LENGTH_LONG,
                ).show()
            } else if (ok) {
                RecorderService.start(this@MainActivity)
                screen = Screen.RECORDING
            } else if (!shouldShowRequestPermissionRationale(Manifest.permission.RECORD_AUDIO)) {
                // Refused for good: Android shows no dialog any more, so say why and open settings.
                Toast.makeText(this@MainActivity, R.string.mic_needed, Toast.LENGTH_LONG).show()
                openAppSettings()
            }
        }

        fun openMeeting(e: ListEntry) {
            openedFrom = if (screen == Screen.UPLOADS) Screen.UPLOADS else Screen.HOME
            openEntry = e.key
            openAudio(e)
            screen = Screen.MEETING
        }
        fun closeMeeting() {
            closeAudio()
            openEntry = null
            screen = openedFrom
        }
        // While playing, the line follows the recording.
        LaunchedEffect(playState.playing) {
            while (playState.playing) {
                kotlinx.coroutines.delay(250)
                playState = PlayState(player.playing, player.positionMs, player.durationMs)
            }
        }

        BackHandler(enabled = screen !in setOf(Screen.HOME, Screen.ONBOARDING)) {
            when (screen) {
                Screen.LICENSES -> screen = Screen.SETTINGS
                Screen.MEETING -> closeMeeting()
                else -> screen = Screen.HOME
            }
        }

        // Home and recording lay themselves out in two panes when wide; the
        // reading screens keep a comfortable width, centred.
        val reading = screen !in setOf(Screen.HOME, Screen.RECORDING)
        Box(
            Modifier.fillMaxSize().background(pal.bg).safeDrawingPadding(),
            contentAlignment = Alignment.TopCenter,
        ) {
          Box(if (reading) Modifier.widthIn(max = ReadingWidth).fillMaxHeight() else Modifier.fillMaxSize()) {
            when (screen) {
                Screen.ONBOARDING -> OnboardingScreen(
                    perms = perms,
                    account = account,
                    onMic = { request(Manifest.permission.RECORD_AUDIO) },
                    onNotifications = { request(Manifest.permission.POST_NOTIFICATIONS) },
                    onCalendar = { request(Manifest.permission.READ_CALENDAR) },
                    onLogin = ::login,
                    loginMessage = loginMessage,
                    onStart = {
                        settings.onboarded = true
                        screen = Screen.HOME
                    },
                )
                Screen.HOME -> HomeScreen(
                    rec = rec,
                    event = CalendarPick.next(events, now),
                    now = now,
                    recordings = recordings,
                    entries = entries,
                    wifiOnly = wifiOnly,
                    signedIn = account != null,
                    onRecord = {
                        when {
                            rec !is RecState.Idle -> screen = Screen.RECORDING
                            !perms.mic -> askMicThenRecord.launch(Manifest.permission.RECORD_AUDIO)
                            !Space.canStart(freeBytes()) -> Toast.makeText(
                                this@MainActivity, getString(R.string.no_space_to_start, (freeBytes() / Space.MB).toInt()), Toast.LENGTH_LONG,
                            ).show()
                            else -> {
                                RecorderService.start(this@MainActivity)
                                screen = Screen.RECORDING
                            }
                        }
                    },
                    onDictation = { screen = Screen.DICTATION },
                    onSettings = { screen = Screen.SETTINGS },
                    onUploads = { screen = Screen.UPLOADS },
                    onRetry = ::retry,
                    onLogin = ::login,
                    onDelete = { deleteTarget = it },
                    saveProblem = saveProblem,
                    onOpen = ::openMeeting,
                )
                Screen.RECORDING -> RecordingScreen(
                    rec = rec,
                    onBack = { screen = Screen.HOME },
                    onMark = { RecorderService.mark(this@MainActivity) },
                    onStop = { RecorderService.stop(this@MainActivity) },
                    onDiscard = { RecorderService.discard(this@MainActivity) },
                )
                Screen.UPLOADS -> UploadsScreen(
                    recordings = recordings,
                    entries = entries,
                    account = account,
                    wifiOnly = wifiOnly,
                    now = now,
                    onBack = { screen = Screen.HOME },
                    onRetry = ::retry,
                    onOpenFolder = {
                        settings.driveFolderId?.let { open("https://drive.google.com/drive/folders/$it") }
                    },
                    onLogin = ::login,
                    onWifiOnly = ::setWifiOnly,
                    onDelete = { deleteTarget = it },
                    onOpen = ::openMeeting,
                    listProblem = when (meetingsProblem) {
                        MeetingLibrary.Outcome.OFFLINE -> stringResource(R.string.list_offline)
                        MeetingLibrary.Outcome.FAILED -> stringResource(R.string.list_failed)
                        MeetingLibrary.Outcome.NEED_LOGIN -> stringResource(R.string.list_need_login)
                        else -> null
                    },
                    onRefresh = {
                        if (meetingsProblem == MeetingLibrary.Outcome.NEED_LOGIN) login() else app.refreshMeetings(now = true)
                    },
                )
                Screen.MEETING -> {
                    val e = entries.firstOrNull { it.key == openEntry }
                    val m = e?.meeting
                    if (e == null || m == null) {
                        LaunchedEffect(Unit) { closeMeeting() }
                    } else {
                        // Back from the background: the recording's place is found again.
                        LaunchedEffect(e.key) { openAudio(e) }
                        MeetingScreen(
                            title = m.title,
                            startedAt = m.startedAtMs,
                            durationMs = m.durationMs,
                            fromPhone = m.origin == "phone",
                            doc = remember(m.id, m.jsonVersion, m.jsonFileId) { app.meetings.doc(m.id) },
                            now = now,
                            audio = audioState,
                            play = playState,
                            onBack = ::closeMeeting,
                            onPlayAt = { ms -> playAt(e, ms) },
                            onPause = {
                                player.pause()
                                playState = playState.copy(playing = false)
                            },
                        )
                    }
                }
                Screen.SETTINGS -> SettingsScreen(
                    account = account,
                    wifiOnly = wifiOnly,
                    calendar = perms.calendar,
                    onBack = { screen = Screen.HOME },
                    onLogin = { if (account == null) login() else switchAccount() },
                    loginMessage = loginMessage,
                    onWifiOnly = ::setWifiOnly,
                    onCalendar = { request(Manifest.permission.READ_CALENDAR) },
                    onPrivacy = { open(PRIVACY_URL) },
                    onFeedback = { open(FEEDBACK_URL) },
                    onLicenses = { screen = Screen.LICENSES },
                )
                Screen.DICTATION -> DictationScreen(onBack = { screen = Screen.HOME })
                Screen.LICENSES -> LicensesScreen(onBack = { screen = Screen.SETTINGS })
            }
          }
            if (pending != null) {
                UndoBar(
                    stringResource(R.string.deleted), stringResource(R.string.undo), { app.deletes.undo() },
                    Modifier.align(Alignment.BottomCenter).widthIn(max = ReadingWidth),
                )
            }
        }
        deleteTarget?.let { r ->
            val uploaded = Deleter.offerDriveTrash(r, app.settings.driveFolderId)
            ConfirmDialog(
                title = stringResource(R.string.delete_title),
                body = stringResource(R.string.delete_body),
                confirm = stringResource(R.string.delete_confirm),
                cancel = stringResource(R.string.cancel),
                check = if (uploaded) stringResource(R.string.delete_trash_drive) else null,
                checkedAtFirst = true,
                onConfirm = { trash ->
                    deleteTarget = null
                    startDelete(r, trash && uploaded)
                },
                onDismiss = { deleteTarget = null },
            )
        }
    }

    private companion object {
        const val PRIVACY_URL = "https://lokim-dj.github.io/seogi/privacy/"
        const val FEEDBACK_URL = "https://github.com/lokim-dj/seogi/issues/new/choose"
    }
}
