package io.github.lokimdj.seogi

import android.app.Application
import android.net.Uri
import io.github.lokimdj.seogi.data.RecordingStore
import io.github.lokimdj.seogi.data.Settings
import io.github.lokimdj.seogi.record.Finalizer
import io.github.lokimdj.seogi.record.Notifications
import io.github.lokimdj.seogi.record.RecorderService
import io.github.lokimdj.seogi.record.Saver
import io.github.lokimdj.seogi.record.SessionDir
import io.github.lokimdj.seogi.upload.DeleteWindow
import io.github.lokimdj.seogi.upload.Deleter
import io.github.lokimdj.seogi.upload.DriveApi
import io.github.lokimdj.seogi.upload.GoogleAuth
import io.github.lokimdj.seogi.upload.TokenResult
import io.github.lokimdj.seogi.upload.UploadQueue
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.launch
import java.io.File
import java.time.ZoneId

class SeogiApp : Application() {
    lateinit var store: RecordingStore
        private set
    lateinit var settings: Settings
        private set
    val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)

    /** Recordings in their few seconds of "되돌리기": hidden, and not uploaded. */
    val pendingDeletes = MutableStateFlow<Set<String>>(emptySet())

    override fun onCreate() {
        super.onCreate()
        store = RecordingStore(File(filesDir, "recordings.json"))
        settings = Settings(this)
        Notifications.createChannels(this)
        recoverLeftovers()
        UploadQueue.enqueue(this, UploadQueue.Mode.KEEP)
    }

    /** A recording could not be saved; it stays for the next start. Shown in red on the home. */
    val saveProblem = MutableStateFlow(false)

    private val saver by lazy {
        Saver(
            store = store,
            finalize = { stream, state, recovered, onPending -> Finalizer.finalize(this, stream, state, recovered, onPending) },
            discardPublished = { Finalizer.discard(this, it) },
            enqueue = { UploadQueue.enqueue(this, UploadQueue.Mode.APPEND) },
            onFailure = { id, e ->
                android.util.Log.w("Seogi", "saving $id failed; kept for the next start", e)
                saveProblem.value = true
                Notifications.saveFailed(this)
            },
            onSuccess = { saveProblem.value = false },
        )
    }

    /** Saves a stopped (or cut off) recording exactly once; see [Saver]. */
    suspend fun saveSession(sessions: SessionDir, id: String, recovered: Boolean) =
        saver.save(sessions, id, recovered, ZoneId.systemDefault().id)

    /** Deletes a recording the user confirmed: Drive trash (optional), phone file, list line. */
    fun deleter() = Deleter(
        store = store,
        drive = DriveApi(),
        tokens = { settings.email.value?.let { GoogleAuth.token(this, it.ifEmpty { null }) } ?: TokenResult.NeedLogin },
        currentAccount = { settings.email.value },
        deleteOnPhone = { r ->
            val uri = r.mediaUri?.let(Uri::parse)
            when {
                uri == null -> Deleter.PhoneResult.ALREADY_GONE
                else -> try {
                    if (contentResolver.delete(uri, null, null) > 0) Deleter.PhoneResult.DELETED else Deleter.PhoneResult.ALREADY_GONE
                } catch (e: SecurityException) {
                    Deleter.PhoneResult.FAILED
                }
            }
        },
    )

    /**
     * The undo window for deletes. It lives here, not in the screen, so a
     * rotation keeps it; leaving the app commits it.
     */
    val deletes = DeleteWindow(
        scope = scope,
        onHide = { pendingDeletes.value = pendingDeletes.value + it },
        onShow = {
            pendingDeletes.value = pendingDeletes.value - it
            UploadQueue.enqueue(this, UploadQueue.Mode.APPEND)
        },
        commit = { p ->
            val current = store.get(p.recording.id) ?: p.recording
            val result = try {
                deleter().delete(current, p.trashInDrive)
            } finally {
                pendingDeletes.value = pendingDeletes.value - p.recording.id
                UploadQueue.enqueue(this, UploadQueue.Mode.APPEND)
            }
            val problem = when {
                !result.phoneDeleted -> getString(R.string.delete_phone_failed)
                !result.listRemoved -> getString(R.string.delete_list_failed)
                result.drive == Deleter.DriveResult.OTHER_ACCOUNT -> getString(R.string.delete_other_account, result.otherAccount)
                result.drive == Deleter.DriveResult.FAILED -> getString(R.string.delete_drive_failed)
                else -> null
            }
            if (problem != null) {
                kotlinx.coroutines.withContext(Dispatchers.Main) {
                    android.widget.Toast.makeText(this@SeogiApp, problem, android.widget.Toast.LENGTH_LONG).show()
                }
            }
        },
    )

    /** Finishes recordings a crash left behind. Safe to call any time. */
    fun recoverLeftovers() {
        scope.launch {
            val sessions = SessionDir(RecorderService.sessionDir(this@SeogiApp))
            sessions.leftovers(RecorderService.activeId, ZoneId.systemDefault().id).forEach {
                if (it.id != RecorderService.activeId) saveSession(sessions, it.id, recovered = true)
            }
        }
    }
}
