package io.github.lokimdj.seogi

import android.app.Application
import android.net.Uri
import io.github.lokimdj.seogi.data.RecordingStore
import io.github.lokimdj.seogi.data.Settings
import io.github.lokimdj.seogi.record.Finalizer
import io.github.lokimdj.seogi.record.Notifications
import io.github.lokimdj.seogi.record.RecorderService
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
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
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
    private val saving = Mutex()

    override fun onCreate() {
        super.onCreate()
        store = RecordingStore(File(filesDir, "recordings.json"))
        settings = Settings(this)
        Notifications.createChannels(this)
        recoverLeftovers()
        UploadQueue.enqueue(this, UploadQueue.Mode.KEEP)
    }

    /**
     * Saves a stopped (or cut off) recording to `Recordings/서기/`, lists it
     * and queues its upload. The stream is removed only once that worked;
     * otherwise it stays for the next start to try again.
     */
    suspend fun saveSession(sessions: SessionDir, id: String, recovered: Boolean) = saving.withLock {
        // Already in MediaStore (and maybe listed) before a crash: list it, never copy twice.
        sessions.readSaved(id)?.let {
            if (store.get(id) == null) store.upsert(it)
            sessions.clear(id)
            UploadQueue.enqueue(this, UploadQueue.Mode.APPEND)
            return@withLock
        }
        store.get(id)?.takeIf { it.mediaUri != null }?.let {
            sessions.clear(id)
            return@withLock
        }
        val stream = sessions.audio(id)
        if (!stream.exists()) return@withLock
        if (stream.length() == 0L) {
            sessions.clear(id)
            return@withLock
        }
        val state = sessions.read(id)
            ?: sessions.leftovers(activeId = null, zone = ZoneId.systemDefault().id).firstOrNull { it.id == id }
            ?: return@withLock
        try {
            val recording = Finalizer.finalize(this, stream, state, recovered)
            sessions.writeSaved(recording)
            store.upsert(recording)
            sessions.clear(id)
            UploadQueue.enqueue(this, UploadQueue.Mode.APPEND)
        } catch (e: Exception) {
            android.util.Log.w("Seogi", "saving $id failed; kept for the next start", e)
        }
    }

    /** Deletes a recording the user confirmed: Drive trash (optional), phone file, list line. */
    fun deleter() = Deleter(
        store = store,
        drive = DriveApi(),
        tokens = { settings.email.value?.let { GoogleAuth.token(this, it.ifEmpty { null }) } ?: TokenResult.NeedLogin },
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
                !result.phoneDeleted -> R.string.delete_phone_failed
                result.drive == Deleter.DriveResult.FAILED -> R.string.delete_drive_failed
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
