package io.github.lokimdj.seogi.dictation

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock

/**
 * Something slow to open and heavy to keep (the speech model): opened on first
 * use, kept while it is used, closed after [idleMs] without use. Every use and
 * the closing take turns, so the engine is never closed under a running
 * transcription and never used from two places at once.
 */
class Loaded<T : AutoCloseable>(
    private val scope: CoroutineScope,
    private val idleMs: Long,
    private val open: () -> T,
) {
    private val turn = Mutex()
    private var value: T? = null
    private var timer: Job? = null

    val isOpen: Boolean get() = value != null

    /** Runs [block] with the thing, opening it first if needed. */
    suspend fun <R> use(block: (T) -> R): R = turn.withLock {
        timer?.cancel()
        try {
            block(value ?: open().also { value = it })
        } finally {
            timer = scope.launch {
                delay(idleMs)
                turn.withLock { closeNow() }
            }
        }
    }

    /** Opens it ahead of use (while the user is still speaking). */
    suspend fun warm() = use { }

    /** Closes it now, waiting for a use under way. */
    suspend fun close() = turn.withLock {
        timer?.cancel()
        closeNow()
    }

    private fun closeNow() {
        val v = value ?: return
        value = null
        try { v.close() } catch (_: Exception) {}
    }
}
