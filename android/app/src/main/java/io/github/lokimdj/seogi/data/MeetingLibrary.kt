package io.github.lokimdj.seogi.data

import io.github.lokimdj.seogi.upload.DriveHttpException
import io.github.lokimdj.seogi.upload.DriveItem
import io.github.lokimdj.seogi.upload.DriveReader
import io.github.lokimdj.seogi.upload.TokenResult
import io.github.lokimdj.seogi.upload.Uploader
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.serialization.Serializable
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.json.Json
import java.io.File
import java.io.IOException

/**
 * One meeting in the shared Drive folder whose minutes the computer wrote,
 * as the phone's list shows it. Its `회의.json` is kept beside the list, so
 * the minutes open without a network.
 */
@Serializable
data class DriveMeeting(
    val id: String,
    val folderId: String,
    val folderName: String,
    val jsonFileId: String,
    /** Drive's version of `회의.json` when it was read; a newer one is read again. */
    val jsonVersion: Long? = null,
    val title: String,
    val startedAtMs: Long,
    val durationMs: Long,
    /** `recorded`, `imported`, `watched` or `phone`. */
    val origin: String,
    val tasks: Int = 0,
    val openTasks: Int = 0,
    /** The recording in the meeting's folder, downloaded only when the user plays it. */
    val audioFileId: String? = null,
    val audioName: String? = null,
    val audioSize: Long? = null,
    /** For a meeting recorded on this phone: the recording's id here. */
    val phoneRecordingId: String? = null,
)

/**
 * The meetings in the shared Drive folder (issue #67): every meeting folder
 * the computer made, its own recordings' and the phone's. [refresh] lists the
 * folder and reads each `회의.json` that is new or changed; the recordings
 * stay in Drive until the user plays one ([audio]).
 *
 * Nothing here writes to Drive except making the shared folder when there is
 * none yet, as uploading does.
 */
class MeetingLibrary(
    private val dir: File,
    private val reader: DriveReader,
    private val tokens: suspend () -> TokenResult,
    private val folder: Uploader.FolderCache,
) {
    enum class Outcome { DONE, OFFLINE, NEED_LOGIN, FAILED }

    private val json = Json { ignoreUnknownKeys = true; encodeDefaults = true }
    private val listSerializer = ListSerializer(DriveMeeting.serializer())
    private val indexFile = File(dir, INDEX)
    private val lock = Mutex()
    private val docs = HashMap<String, MeetingDoc>()

    private val state = MutableStateFlow(load())

    /** Newest first. */
    val all: StateFlow<List<DriveMeeting>> = state.asStateFlow()

    private fun load(): List<DriveMeeting> = try {
        if (indexFile.exists()) json.decodeFromString(listSerializer, indexFile.readText()) else emptyList()
    } catch (e: Exception) {
        emptyList() // a list that cannot be read is made again by the next refresh
    }

    fun get(id: String): DriveMeeting? = state.value.firstOrNull { it.id == id }

    /** The meeting's minutes as last read; null when not read yet. */
    fun doc(id: String): MeetingDoc? = synchronized(docs) {
        docs[id] ?: docFile(id).takeIf { it.exists() }?.let { f ->
            MeetingDoc.parse(f.readText())?.also { docs[id] = it }
        }
    }

    private fun docFile(id: String) = File(dir, "${safe(id)}.json")

    /** Lists the shared folder and reads what changed. Never throws. */
    suspend fun refresh(): Outcome = lock.withLock {
        val token = when (val t = tokens()) {
            is TokenResult.Ok -> t.token
            TokenResult.NeedLogin -> return@withLock Outcome.NEED_LOGIN
        }
        try {
            sync(token)
            Outcome.DONE
        } catch (e: IOException) {
            Outcome.OFFLINE
        } catch (e: DriveHttpException) {
            if (e.code == 401 || e.code == 403) Outcome.NEED_LOGIN else Outcome.FAILED
        } catch (e: Exception) {
            Outcome.FAILED
        }
    }

    private fun sync(token: String) {
        val root = rootFolder(token)
        val folders = reader.folders(token, root).filter { it.meetingId != null }
        val items = if (folders.isEmpty()) emptyList() else reader.filesIn(token, folders.map { it.id })
        val byFolder = items.groupBy { it.parent }
        val before = state.value.associateBy { it.id }
        val found = mutableListOf<DriveMeeting>()
        for (f in folders) {
            val files = byFolder[f.id].orEmpty()
            // Written last by the computer: a folder without it is not ready.
            val record = files.firstOrNull { it.name == MEETING_JSON } ?: continue
            val known = before[f.meetingId]
            val doc = if (known != null && known.jsonFileId == record.id && known.jsonVersion != null &&
                known.jsonVersion == record.version && docFile(known.id).exists()
            ) {
                doc(known.id)
            } else {
                null
            } ?: read(token, record) ?: continue
            if (doc.id != f.meetingId) continue
            val audio = audioIn(files, doc)
            found += DriveMeeting(
                id = doc.id,
                folderId = f.id,
                folderName = f.name,
                jsonFileId = record.id,
                jsonVersion = record.version,
                title = doc.title.ifBlank { f.name },
                startedAtMs = doc.startedAtMs,
                durationMs = doc.durationMs,
                origin = doc.origin,
                tasks = doc.tasks.size,
                openTasks = doc.tasks.count { !it.done },
                audioFileId = audio?.id,
                audioName = audio?.name,
                audioSize = audio?.size,
                phoneRecordingId = audio?.recordingId ?: files.firstNotNullOfOrNull { it.recordingId },
            )
        }
        val list = found.distinctBy { it.id }.sortedByDescending { it.startedAtMs }
        // A meeting taken out of Drive (its folder in the trash) leaves the list here too.
        val gone = before.keys - list.map { it.id }.toSet()
        gone.forEach { id ->
            docFile(id).delete()
            synchronized(docs) { docs.remove(id) }
        }
        save(list)
    }

    /** The saved shared folder while it is usable; else the marked one, found (or made) again. */
    private fun rootFolder(token: String): String {
        folder.folderId?.let { if (reader.folderUsable(token, it)) return it }
        return reader.ensureFolder(token).also { folder.folderId = it }
    }

    private fun read(token: String, record: DriveItem): MeetingDoc? {
        val text = reader.readText(token, record.id)
        val doc = MeetingDoc.parse(text) ?: return null
        writeAtomically(docFile(doc.id), text)
        synchronized(docs) { docs[doc.id] = doc }
        return doc
    }

    private fun save(list: List<DriveMeeting>) {
        writeAtomically(indexFile, json.encodeToString(listSerializer, list))
        state.value = list
    }

    private fun writeAtomically(to: File, text: String) {
        dir.mkdirs()
        val tmp = File(dir, to.name + ".tmp")
        tmp.writeText(text)
        if (!tmp.renameTo(to)) {
            to.delete()
            if (!tmp.renameTo(to)) throw IOException("could not save ${to.name}")
        }
    }

    /**
     * The meeting's recording on this phone, downloaded from Drive the first
     * time (into [cacheDir]; a download cut off goes on from where it
     * stopped). The file in Drive is only read.
     */
    suspend fun audio(id: String, cacheDir: File, onProgress: (Float) -> Unit): File? {
        val m = get(id) ?: return null
        val fileId = m.audioFileId ?: return null
        val token = (tokens() as? TokenResult.Ok)?.token ?: throw NeedLogin()
        val folder = File(cacheDir, AUDIO_DIR).apply { mkdirs() }
        val ext = m.audioName?.substringAfterLast('.', "")?.takeIf { it.isNotEmpty() && it.length <= 5 } ?: "audio"
        val done = File(folder, "${safe(fileId)}.$ext")
        if (done.exists() && (m.audioSize == null || done.length() == m.audioSize)) return done
        val part = File(folder, "${safe(fileId)}.$ext.part")
        reader.download(token, fileId, part) { got, total ->
            val whole = m.audioSize ?: total
            if (whole > 0) onProgress((got.toFloat() / whole).coerceIn(0f, 1f))
        }
        if (m.audioSize != null && part.length() != m.audioSize) throw IOException("download cut off")
        if (!part.renameTo(done)) throw IOException("could not keep the download")
        return done
    }

    /** Signed out while playing: the caller asks the user to sign in. */
    class NeedLogin : Exception()

    companion object {
        const val MEETING_JSON = "회의.json"
        private const val INDEX = "meetings.json"
        private const val AUDIO_DIR = "meeting-audio"

        /** Text files the computer writes; anything else in a meeting's folder is its recording. */
        private val TEXT_ENDINGS = listOf(".json", ".txt", ".md")

        /** A name safe as a file name here. */
        internal fun safe(s: String) = s.replace(Regex("[^A-Za-z0-9._-]"), "_")

        /** The recording in a meeting's folder: the one `회의.json` names, else the only other file. */
        internal fun audioIn(files: List<DriveItem>, doc: MeetingDoc): DriveItem? {
            val others = files.filter { f -> TEXT_ENDINGS.none { f.name.lowercase().endsWith(it) } }
            return others.firstOrNull { it.name == doc.audio.file } ?: others.singleOrNull()
        }
    }
}
