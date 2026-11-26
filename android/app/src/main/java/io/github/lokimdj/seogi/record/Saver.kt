package io.github.lokimdj.seogi.record

import io.github.lokimdj.seogi.data.Recording
import io.github.lokimdj.seogi.data.RecordingStore
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import java.io.File

/**
 * Saves a stopped (or cut off) recording to `Recordings/서기/`, lists it and
 * queues its upload, exactly once. The stream is removed only after that
 * worked; otherwise it stays for the next start, and [onFailure] tells the
 * user. A crash at any step leaves enough behind to finish without a second
 * copy: the MediaStore address is noted before writing, the finished
 * recording before listing.
 */
class Saver(
    private val store: RecordingStore,
    private val finalize: (stream: File, state: SessionState, recovered: Boolean, onPending: (String) -> Unit) -> Recording,
    private val discardPublished: (uri: String) -> Unit,
    private val enqueue: () -> Unit,
    /** [first] is true only for a recording's first failed save, so the user is told once. */
    private val onFailure: (id: String, e: Exception, first: Boolean) -> Unit,
    private val onSuccess: () -> Unit,
) {
    private val lock = Mutex()

    suspend fun save(sessions: SessionDir, id: String, recovered: Boolean, zone: String) = lock.withLock {
        try {
            // Finished before a crash, not yet listed: list it, never copy twice.
            sessions.readSaved(id)?.let {
                if (store.get(id) == null) store.upsert(it)
                sessions.clear(id)
                enqueue()
                onSuccess()
                return@withLock
            }
            store.get(id)?.takeIf { it.mediaUri != null }?.let {
                sessions.clear(id)
                return@withLock
            }
            // A MediaStore item from a save that was cut off: its content is not
            // known to be whole, so it goes and the save starts over.
            sessions.readPublishing(id)?.let {
                discardPublished(it)
                sessions.clearPublishing(id)
            }
            val stream = sessions.audio(id)
            if (!stream.exists()) return@withLock
            if (stream.length() == 0L) {
                sessions.clear(id)
                return@withLock
            }
            val state = sessions.read(id)
                ?: sessions.leftovers(activeId = null, zone = zone).firstOrNull { it.id == id }
                ?: return@withLock
            val recording = finalize(stream, state, recovered) { uri -> sessions.writePublishing(id, uri) }
            sessions.writeSaved(recording)
            sessions.clearPublishing(id)
            store.upsert(recording)
            sessions.clear(id)
            enqueue()
            onSuccess()
        } catch (e: Exception) {
            val first = try { sessions.noteFailure(id) } catch (_: Exception) { true }
            onFailure(id, e, first)
        }
    }
}
