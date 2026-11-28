package io.github.lokimdj.seogi

import android.app.Application
import android.net.Uri
import android.provider.MediaStore
import io.github.lokimdj.seogi.data.ListEntry
import io.github.lokimdj.seogi.data.MeetingLibrary
import io.github.lokimdj.seogi.data.MeetingLook
import io.github.lokimdj.seogi.data.PhoneFile
import io.github.lokimdj.seogi.data.Reconcile
import io.github.lokimdj.seogi.data.RecordingStore
import io.github.lokimdj.seogi.data.Settings
import io.github.lokimdj.seogi.dictation.A11yWatch
import io.github.lokimdj.seogi.dictation.DictationPrefs
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
import io.github.lokimdj.seogi.upload.Uploader
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
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
        // The floating key's process (`:dictation`) shares this class but none
        // of the recording and upload work: only the main process touches them.
        if (getProcessName() != packageName) return
        store = RecordingStore(File(filesDir, "recordings.json"))
        settings = Settings(this)
        Notifications.createChannels(this)
        if (store.restoredFromBackup) scope.launch { reconcileWithPhone() }
        recoverLeftovers()
        UploadQueue.enqueue(this, UploadQueue.Mode.KEEP)
        // The floating key's accessibility check (#98), for keys left on before it existed.
        if (DictationPrefs.isOn(this)) {
            A11yWatch.schedule(this)
            scope.launch { A11yWatch.check(this@SeogiApp) }
        }
    }

    /** The meetings in the shared Drive folder, the computer's too (issue #67). */
    val meetings by lazy {
        MeetingLibrary(
            dir = File(filesDir, "meetings"),
            reader = DriveApi(),
            tokens = { settings.email.value?.let { GoogleAuth.token(this, it.ifEmpty { null }) } ?: TokenResult.NeedLogin },
            folder = object : Uploader.FolderCache {
                override var folderId: String?
                    get() = settings.driveFolderId
                    set(v) { settings.driveFolderId = v }
            },
        )
    }

    /** How the last look at the shared folder went; null while it went fine. */
    val meetingsProblem = MutableStateFlow<MeetingLibrary.Outcome?>(null)
    private val look = MeetingLook()

    /** A look the user pulled the list for is under way: the lists' ring turns (issue #102). */
    val readingMeetings: StateFlow<Boolean> get() = look.pulled

    /**
     * Looks at the shared Drive folder for new or changed meetings, at most
     * once a minute unless [now]. Does nothing before the first sign-in. A
     * [pull] on the list while a look runs is dropped; any other call with
     * [now] runs once that look ends.
     */
    fun refreshMeetings(now: Boolean = false, pull: Boolean = false) {
        if (!look.start(settings.email.value != null, System.currentTimeMillis(), now, pull)) return
        scope.launch {
            var again = false
            val outcome = try {
                meetings.refresh()
            } finally {
                again = look.done()
            }
            meetingsProblem.value = outcome.takeIf { it != MeetingLibrary.Outcome.DONE }
            if (outcome == MeetingLibrary.Outcome.DONE) meetings.pushEdits()
            if (again) refreshMeetings(now = true)
        }
    }

    /**
     * A change made on the phone (a task ticked, a line fixed, a speaker
     * named): kept at once, and sent to the meeting's Drive folder for the
     * computer. Changes waiting go again at the next look at the folder.
     */
    fun editMeeting(id: String, key: String, value: kotlinx.serialization.json.JsonElement) {
        meetings.edit(id, key, value)
        scope.launch { meetings.pushEdits() }
    }

    /** Meetings being deleted right now: hidden from the lists. */
    val deletingMeetings = MutableStateFlow<Set<String>>(emptySet())

    /**
     * Deletes a meeting on the phone (decision 5 of #67): its folder in
     * Drive, with the recording and the minutes, goes to Drive's trash, and
     * the phone's own recording of it (if any) is deleted here. The
     * computer keeps its copy. Says what went wrong, if anything.
     */
    fun deleteMeeting(e: ListEntry) {
        val m = e.meeting ?: return
        deletingMeetings.value = deletingMeetings.value + m.id
        scope.launch {
            val outcome = try {
                meetings.trashMeeting(m.id)
            } finally {
                deletingMeetings.value = deletingMeetings.value - m.id
            }
            var problem = when (outcome) {
                MeetingLibrary.Outcome.DONE -> null
                MeetingLibrary.Outcome.OFFLINE -> getString(R.string.delete_meeting_offline)
                MeetingLibrary.Outcome.NEED_LOGIN -> getString(R.string.delete_meeting_login)
                MeetingLibrary.Outcome.FAILED -> getString(R.string.delete_drive_failed)
            }
            val r = e.recording
            if (problem == null && r != null) {
                // Its copy in Drive went with the folder; only the phone's file is left.
                val result = deleter().delete(store.get(r.id) ?: r, trashInDrive = false)
                if (!result.phoneDeleted) problem = getString(R.string.delete_phone_failed)
            }
            if (problem != null) {
                kotlinx.coroutines.withContext(Dispatchers.Main) {
                    android.widget.Toast.makeText(this@SeogiApp, problem, android.widget.Toast.LENGTH_LONG).show()
                }
            }
        }
    }

    /** A recording could not be saved; it stays for the next start. Shown in red on the home. */
    val saveProblem = MutableStateFlow(false)

    private val saver by lazy {
        Saver(
            store = store,
            finalize = { stream, state, recovered, onPending -> Finalizer.finalize(this, stream, state, recovered, onPending) },
            discardPublished = { Finalizer.discard(this, it) },
            enqueue = { UploadQueue.enqueue(this, UploadQueue.Mode.APPEND) },
            onFailure = { id, e, first ->
                android.util.Log.w("Seogi", "saving $id failed; kept for the next start", e)
                saveProblem.value = true
                if (first) Notifications.saveFailed(this)
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
        folderId = { settings.driveFolderId },
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
                result.drive == Deleter.DriveResult.NOT_FOUND -> getString(R.string.delete_drive_not_found)
                else -> null
            }
            if (problem != null) {
                kotlinx.coroutines.withContext(Dispatchers.Main) {
                    android.widget.Toast.makeText(this@SeogiApp, problem, android.widget.Toast.LENGTH_LONG).show()
                }
            }
        },
    )

    /** The list came back from its backup: match it with the files really in `Recordings/서기/`. */
    private suspend fun reconcileWithPhone() {
        val files = mutableListOf<PhoneFile>()
        val cols = arrayOf(
            MediaStore.MediaColumns._ID, MediaStore.MediaColumns.DISPLAY_NAME, MediaStore.MediaColumns.MIME_TYPE,
            MediaStore.MediaColumns.SIZE, MediaStore.MediaColumns.DURATION, MediaStore.MediaColumns.DATE_ADDED,
        )
        val collection = MediaStore.Audio.Media.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY)
        try {
            contentResolver.query(
                collection, cols,
                "${MediaStore.MediaColumns.RELATIVE_PATH} = ? AND ${MediaStore.MediaColumns.OWNER_PACKAGE_NAME} = ?",
                arrayOf("${android.os.Environment.DIRECTORY_RECORDINGS}/${Finalizer.FOLDER}/", packageName), null,
            )?.use { c ->
                while (c.moveToNext()) {
                    files += PhoneFile(
                        uri = android.content.ContentUris.withAppendedId(collection, c.getLong(0)).toString(),
                        name = c.getString(1).orEmpty(), mime = c.getString(2) ?: "audio/mp4",
                        sizeBytes = c.getLong(3), durationMs = c.getLong(4), addedAt = c.getLong(5) * 1000,
                    )
                }
            } ?: return
        } catch (e: Exception) {
            return // could not look: keep the backup as it is
        }
        store.replaceAll(Reconcile.merge(store.all.value, files, ZoneId.systemDefault()))
        UploadQueue.enqueue(this, UploadQueue.Mode.APPEND)
    }

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
