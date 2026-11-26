package io.github.lokimdj.seogi.upload

import io.github.lokimdj.seogi.data.Recording
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch

/** A delete the user confirmed, waiting out its "되돌리기" seconds. */
data class PendingDelete(val recording: Recording, val trashInDrive: Boolean)

/**
 * The few seconds between "지우기" and the real delete. While a delete is
 * pending nothing has happened yet: no file, no list line, no Drive trash.
 * [undo] in time and it never happens; otherwise [commit] runs once, when the
 * time is up, when the user leaves the app ([commitNow]), or when another
 * delete starts.
 */
class DeleteWindow(
    private val scope: CoroutineScope,
    private val windowMs: Long = WINDOW_MS,
    private val onHide: (String) -> Unit,
    private val onShow: (String) -> Unit,
    private val commit: suspend (PendingDelete) -> Unit,
) {
    private val _pending = MutableStateFlow<PendingDelete?>(null)
    val pending: StateFlow<PendingDelete?> = _pending.asStateFlow()
    private var timer: Job? = null

    fun start(recording: Recording, trashInDrive: Boolean) {
        commitNow()
        val p = PendingDelete(recording, trashInDrive)
        onHide(recording.id)
        _pending.value = p
        timer = scope.launch {
            delay(windowMs)
            if (_pending.compareAndSet(p, null)) commit(p)
        }
    }

    /** Puts it back; true when there was still something to undo. */
    fun undo(): Boolean {
        val p = _pending.value ?: return false
        if (!_pending.compareAndSet(p, null)) return false
        timer?.cancel()
        onShow(p.recording.id)
        return true
    }

    fun commitNow() {
        val p = _pending.value ?: return
        if (!_pending.compareAndSet(p, null)) return
        timer?.cancel()
        scope.launch { commit(p) }
    }

    companion object {
        const val WINDOW_MS = 8_000L
    }
}
