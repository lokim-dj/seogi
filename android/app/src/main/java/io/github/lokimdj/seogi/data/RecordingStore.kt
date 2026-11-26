package io.github.lokimdj.seogi.data

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.json.Json
import java.io.File

/**
 * The list of finished recordings, kept in one JSON file. A write goes to a
 * temporary file first and is renamed over the old one, so a crash mid-write
 * never leaves half a list. A file that cannot be read is moved aside (never
 * deleted) and the list starts empty.
 */
class RecordingStore(private val file: File) {
    private val json = Json { ignoreUnknownKeys = true; encodeDefaults = true }
    private val serializer = ListSerializer(Recording.serializer())
    private val lock = Mutex()
    private val state = MutableStateFlow(load())

    /** Every recording, newest first. */
    val all: StateFlow<List<Recording>> = state.asStateFlow()

    fun get(id: String): Recording? = state.value.firstOrNull { it.id == id }

    suspend fun upsert(recording: Recording) = lock.withLock {
        val rest = state.value.filterNot { it.id == recording.id }
        save((rest + recording).sortedByDescending { it.startedAt })
    }

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
        tmp.writeText(json.encodeToString(serializer, list))
        if (!tmp.renameTo(file)) {
            file.delete()
            check(tmp.renameTo(file)) { "could not replace ${file.name}" }
        }
        state.value = list
    }

    private fun load(): List<Recording> {
        if (!file.exists()) return emptyList()
        return try {
            json.decodeFromString(serializer, file.readText()).sortedByDescending { it.startedAt }
        } catch (e: Exception) {
            file.renameTo(File(file.path + ".broken-" + System.currentTimeMillis()))
            emptyList()
        }
    }
}
