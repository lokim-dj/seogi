package io.github.lokimdj.seogi.data

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/**
 * When the phone looks at the shared Drive folder again (issue #102): at
 * most once a [minGapMs] on its own, at once when the user pulls the list,
 * never before the first sign-in, and never twice at the same time.
 * [reading] is what the list's ring shows.
 */
class MeetingLook(private val minGapMs: Long = 60_000) {
    private val state = MutableStateFlow(false)

    /** A look at the folder is under way. */
    val reading: StateFlow<Boolean> = state.asStateFlow()

    private var last: Long? = null

    /**
     * Whether to start a look now; when true, [reading] is on until [done].
     * [now] is a pull: the minute between looks does not hold it back.
     */
    @Synchronized
    fun start(signedIn: Boolean, nowMs: Long, now: Boolean): Boolean {
        if (!signedIn || state.value) return false
        val l = last
        if (!now && l != null && nowMs - l < minGapMs) return false
        last = nowMs
        state.value = true
        return true
    }

    /** The look ended, however it went. */
    @Synchronized
    fun done() {
        state.value = false
    }
}
