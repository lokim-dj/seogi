package io.github.lokimdj.seogi.data

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.json.Json
import java.io.File
import java.io.FileOutputStream
import java.io.IOException

/**
 * The list of finished recordings, kept in one JSON file. A write goes to a
 * temporary file, is flushed to the disk, and is renamed over the old one, so
 * a crash mid-write never leaves half a list; the version before stays as a
 * backup. A file that cannot be read is moved aside (never deleted) and the
 * list comes back from the backup. Writes throw IOException when the disk
 * refuses; callers say so.
 */
class RecordingStore(private val file: File) {
    private val json = Json { ignoreUnknownKeys = true; encodeDefaults = true }
    private val serializer = ListSerializer(Recording.serializer())
    private val lock = Mutex()

    /** True when the list had to come back from its backup this start; see [Reconcile]. */
    var restoredFromBackup = false
        private set
    private val state = MutableStateFlow(load())

    /** Every recording, newest first. */
    val all: StateFlow<List<Recording>> = state.asStateFlow()

    fun get(id: String): Recording? = state.value.firstOrNull { it.id == id }

    suspend fun upsert(recording: Recording) = lock.withLock {
        val rest = state.value.filterNot { it.id == recording.id }
        save((rest + recording).sortedByDescending { it.startedAt })
    }

    /** Replaces the whole list, after [Reconcile]. */
    suspend fun replaceAll(list: List<Recording>) = lock.withLock { save(list.sortedByDescending { it.startedAt }) }

    /** Takes a recording off the list (after its file was deleted). */
    suspend fun remove(id: String) = lock.withLock {
        if (state.value.none { it.id == id }) return@withLock
        save(state.value.filterNot { it.id == id })
    }

    /** Changes one recording; does nothing when it is gone. */
    suspend fun update(id: String, change: (Recording) -> Recording) = lock.withLock {
        if (state.value.none { it.id == id }) return@withLock
        save(state.value.map { if (it.id == id) change(it) else it })
    }

    private fun save(list: List<Recording>) {
        file.parentFile?.mkdirs()
        val tmp = File(file.path + ".tmp")
        FileOutputStream(tmp).use { out ->
            out.write(json.encodeToString(serializer, list).toByteArray())
            out.flush()
            out.fd.sync()
        }
        if (file.exists()) file.copyTo(backup, overwrite = true)
        if (!tmp.renameTo(file)) {
            file.delete()
            if (!tmp.renameTo(file)) throw IOException("could not replace ${file.name}")
        }
        state.value = list
    }

    private val backup get() = File(file.path + ".bak")

    private fun load(): List<Recording> {
        if (!file.exists()) return read(backup) ?: emptyList()
        read(file)?.let { return it }
        file.renameTo(File(file.path + ".broken-" + System.currentTimeMillis()))
        return read(backup)?.also { restoredFromBackup = true } ?: emptyList()
    }

    private fun read(f: File): List<Recording>? = try {
        if (f.exists()) json.decodeFromString(serializer, f.readText()).sortedByDescending { it.startedAt } else null
    } catch (e: Exception) {
        null
    }
}
