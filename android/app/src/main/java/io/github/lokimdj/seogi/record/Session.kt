package io.github.lokimdj.seogi.record

import io.github.lokimdj.seogi.data.Recording
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import java.io.File

/**
 * What is known about a recording while it is still running. Written when it
 * starts and again at every important mark, next to the audio stream, so a
 * recording cut off by a crash can be finished on the next start.
 */
@Serializable
data class SessionState(
    val id: String,
    val startedAt: Long,
    val zone: String,
    val eventTitle: String? = null,
    val marksMs: List<Long> = emptyList(),
)

/** The folder (inside the app's own storage) that holds running recordings. */
class SessionDir(val dir: File) {
    private val json = Json { ignoreUnknownKeys = true }

    fun audio(id: String) = File(dir, "$id.aac")
    private fun stateFile(id: String) = File(dir, "$id.json")

    fun write(state: SessionState) {
        dir.mkdirs()
        val tmp = File(dir, "${state.id}.json.tmp")
        tmp.writeText(json.encodeToString(SessionState.serializer(), state))
        if (!tmp.renameTo(stateFile(state.id))) {
            stateFile(state.id).delete()
            tmp.renameTo(stateFile(state.id))
        }
    }

    fun read(id: String): SessionState? = try {
        stateFile(id).takeIf { it.exists() }?.readText()?.let { json.decodeFromString(SessionState.serializer(), it) }
    } catch (e: Exception) {
        null
    }

    private fun savedFile(id: String) = File(dir, "$id.saved.json")
    private fun publishingFile(id: String) = File(dir, "$id.publishing")

    /** The MediaStore item being written for [id], noted before any byte goes in. */
    fun writePublishing(id: String, uri: String) {
        dir.mkdirs()
        publishingFile(id).writeText(uri)
    }

    fun readPublishing(id: String): String? = publishingFile(id).takeIf { it.exists() }?.readText()?.trim()?.ifEmpty { null }

    fun clearPublishing(id: String) {
        publishingFile(id).delete()
    }

    private fun failedFile(id: String) = File(dir, "$id.failed")

    /** Notes a failed save; true only the first time for this recording. */
    fun noteFailure(id: String): Boolean {
        val f = failedFile(id)
        if (f.exists()) return false
        dir.mkdirs()
        f.writeText(System.currentTimeMillis().toString())
        return true
    }

    /**
     * The finished recording, written right after it reached MediaStore and
     * before it is listed, so a crash in between does not save it twice.
     */
    fun writeSaved(recording: Recording) {
        val tmp = File(dir, "${recording.id}.saved.json.tmp")
        tmp.writeText(json.encodeToString(Recording.serializer(), recording))
        if (!tmp.renameTo(savedFile(recording.id))) {
            savedFile(recording.id).delete()
            tmp.renameTo(savedFile(recording.id))
        }
    }

    fun readSaved(id: String): Recording? = try {
        savedFile(id).takeIf { it.exists() }?.readText()?.let { json.decodeFromString(Recording.serializer(), it) }
    } catch (e: Exception) {
        null
    }

    /** Removes the stream and its notes; only after the recording is safely saved and listed. */
    fun clear(id: String) {
        audio(id).delete()
        stateFile(id).delete()
        savedFile(id).delete()
        publishingFile(id).delete()
        failedFile(id).delete()
    }

    /**
     * Streams left behind by a recording that never finished, except [activeId]
     * (the one running now). A stream without its state still counts; its start
     * comes from the id (`rec-<epoch ms>`) or the file's time.
     */
    fun leftovers(activeId: String?, zone: String): List<SessionState> =
        (dir.listFiles() ?: emptyArray())
            .filter { it.isFile && it.name.endsWith(".aac") }
            .map { it.name.removeSuffix(".aac") }
            .filter { it != activeId }
            .sorted()
            .map { id ->
                read(id) ?: SessionState(
                    id = id,
                    startedAt = id.removePrefix("rec-").toLongOrNull() ?: audio(id).lastModified(),
                    zone = zone,
                )
            }

    companion object {
        fun newId(now: Long) = "rec-$now"
    }
}
