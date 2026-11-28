package io.github.lokimdj.seogi.data

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/**
 * When the phone looks at the shared Drive folder again (issue #102): at
 * most once a [minGapMs] on its own, at once when the user pulls the list,
 * never before the first sign-in, and never twice at the same time. A
 * pull while a look runs is dropped; another call with now (after a
 * sign-in) waits and runs when the look ends, so it reads the fresh state.
 * [reading] is what the list's ring shows.
 */
class MeetingLook(private val minGapMs: Long = 60_000) {
    private val state = MutableStateFlow(false)

    /** A look at the folder is under way. */
    val reading: StateFlow<Boolean> = state.asStateFlow()

    private val pulledState = MutableStateFlow(false)

    /** A look the user pulled the list for is under way: only this one shows the ring. */
    val pulled: StateFlow<Boolean> = pulledState.asStateFlow()

    private var last: Long? = null
    private var again = false

    /**
     * Whether to start a look now; when true, [reading] is on until [done].
     * With [now] the minute between looks does not hold it back; [pull]
     * says it came from the user pulling the list.
     */
    @Synchronized
    fun start(signedIn: Boolean, nowMs: Long, now: Boolean, pull: Boolean = false): Boolean {
        if (!signedIn) return false
        if (state.value) {
            if (now && !pull) again = true
            return false
        }
        val l = last
        if (!now && l != null && nowMs - l < minGapMs) return false
        last = nowMs
        state.value = true
        pulledState.value = pull
        return true
    }

    /** The look ended, however it went. True: a call came meanwhile that wants another look now. */
    @Synchronized
    fun done(): Boolean {
        state.value = false
        pulledState.value = false
        return again.also { again = false }
    }
}
