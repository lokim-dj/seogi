package io.github.lokimdj.seogi

import android.Manifest
import android.accounts.AccountManager
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Bundle
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
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LifecycleEventEffect
import androidx.lifecycle.lifecycleScope
import com.google.android.gms.auth.api.identity.AuthorizationResult
import com.google.android.gms.common.api.ApiException
import com.google.android.gms.common.api.CommonStatusCodes
import io.github.lokimdj.seogi.data.Recording
import io.github.lokimdj.seogi.data.UploadError
import io.github.lokimdj.seogi.data.UploadState
import io.github.lokimdj.seogi.record.CalendarEvent
import io.github.lokimdj.seogi.record.CalendarPick
import io.github.lokimdj.seogi.record.CalendarReader
import io.github.lokimdj.seogi.record.RecState
import io.github.lokimdj.seogi.record.RecorderService
import io.github.lokimdj.seogi.ui.DictationScreen
import io.github.lokimdj.seogi.ui.HomeScreen
import io.github.lokimdj.seogi.ui.OnboardingScreen
import io.github.lokimdj.seogi.ui.Perms
import io.github.lokimdj.seogi.ui.ReadingWidth
import io.github.lokimdj.seogi.ui.RecordingScreen
import io.github.lokimdj.seogi.ui.SeogiTheme
import io.github.lokimdj.seogi.ui.SettingsScreen
import io.github.lokimdj.seogi.ui.UploadsScreen
import io.github.lokimdj.seogi.ui.pal
import io.github.lokimdj.seogi.upload.DriveApi
import io.github.lokimdj.seogi.upload.GoogleAuth
import io.github.lokimdj.seogi.upload.UploadQueue
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

private enum class Screen { ONBOARDING, HOME, RECORDING, UPLOADS, SETTINGS, DICTATION }

class MainActivity : ComponentActivity() {
    private val app get() = application as SeogiApp
    private var pickedEmail: String? = null

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
        }
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

    private fun granted(p: String) = checkSelfPermission(p) == PackageManager.PERMISSION_GRANTED

    private fun perms() = Perms(
        mic = granted(Manifest.permission.RECORD_AUDIO),
        notifications = granted(Manifest.permission.POST_NOTIFICATIONS),
        calendar = granted(Manifest.permission.READ_CALENDAR),
    )

    private fun openAppSettings() = startActivity(
        Intent(SystemSettings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.fromParts("package", packageName, null)),
    )

    @Composable
    private fun Root() {
        val settings = app.settings
        var screen by rememberSaveable { mutableStateOf(if (settings.onboarded) Screen.HOME else Screen.ONBOARDING) }
        var perms by remember { mutableStateOf(perms()) }
        var now by remember { mutableLongStateOf(System.currentTimeMillis()) }
        var events by remember { mutableStateOf(emptyList<CalendarEvent>()) }
        val rec by RecorderService.state.collectAsState()
        val recordings by app.store.all.collectAsState()
        val account by settings.email.collectAsState()
        val wifiOnly by settings.wifiOnly.collectAsState()
        val error by RecorderService.error.collectAsState()

        fun refresh() {
            perms = perms()
            now = System.currentTimeMillis()
            lifecycleScope.launch { events = withContext(Dispatchers.IO) { CalendarReader.today(this@MainActivity) } }
        }
        LifecycleEventEffect(Lifecycle.Event.ON_RESUME) { refresh() }
        LaunchedEffect(Unit) {
            while (true) {
                kotlinx.coroutines.delay(30_000)
                now = System.currentTimeMillis()
            }
        }
        LaunchedEffect(error) {
            error?.let {
                Toast.makeText(this@MainActivity, it, Toast.LENGTH_LONG).show()
                RecorderService.clearError()
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
            if (ok) {
                RecorderService.start(this@MainActivity)
                screen = Screen.RECORDING
            } else if (!shouldShowRequestPermissionRationale(Manifest.permission.RECORD_AUDIO)) {
                // Refused for good: Android shows no dialog any more, so say why and open settings.
                Toast.makeText(this@MainActivity, R.string.mic_needed, Toast.LENGTH_LONG).show()
                openAppSettings()
            }
        }

        BackHandler(enabled = screen !in setOf(Screen.HOME, Screen.ONBOARDING)) { screen = Screen.HOME }

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
                    wifiOnly = wifiOnly,
                    signedIn = account != null,
                    onRecord = {
                        when {
                            rec !is RecState.Idle -> screen = Screen.RECORDING
                            !perms.mic -> askMicThenRecord.launch(Manifest.permission.RECORD_AUDIO)
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
                )
                Screen.RECORDING -> RecordingScreen(
                    rec = rec,
                    onBack = { screen = Screen.HOME },
                    onMark = { RecorderService.mark(this@MainActivity) },
                    onStop = { RecorderService.stop(this@MainActivity) },
                )
                Screen.UPLOADS -> UploadsScreen(
                    recordings = recordings,
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
                )
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
                )
                Screen.DICTATION -> DictationScreen(onBack = { screen = Screen.HOME })
            }
          }
        }
    }

    private companion object {
        const val PRIVACY_URL = "https://github.com/lokim-dj/seogi/blob/main/docs/privacy.md"
        const val FEEDBACK_URL = "https://github.com/lokim-dj/seogi/issues/new/choose"
    }
}
