package io.github.lokimdj.seogi

import android.app.ApplicationExitInfo
import io.github.lokimdj.seogi.dictation.A11yHealth
import io.github.lokimdj.seogi.dictation.A11yLog
import io.github.lokimdj.seogi.dictation.A11yStatus
import io.github.lokimdj.seogi.dictation.ExitNote
import io.github.lokimdj.seogi.dictation.KeyNotice
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** Issue #98: the accessibility connection that Android drops without a word. */
class A11yHealthTest {
    private val grace = A11yHealth.GRACE_MS

    private fun status(
        keyOn: Boolean = true,
        enabled: Boolean = true,
        bound: Boolean = false,
        wasOn: Boolean = true,
        unboundForMs: Long? = null,
    ) = A11yHealth.status(keyOn, enabled, bound, wasOn, unboundForMs)

    // --- where it stands ---

    @Test fun boundIsFine() {
        assertEquals(A11yStatus.FINE, status(bound = true))
        // Bound wins even with the key off or the setting read a moment late.
        assertEquals(A11yStatus.FINE, status(keyOn = false, enabled = false, bound = true))
    }

    /** The case on the Galaxy: still on in the settings, Android no longer connects it ("작동 안 함"). */
    @Test fun onInSettingsButNotBoundIsDroppedAfterTheGrace() {
        assertEquals(A11yStatus.CONNECTING, status(unboundForMs = 0))
        assertEquals(A11yStatus.CONNECTING, status(unboundForMs = grace - 1))
        assertEquals(A11yStatus.DROPPED, status(unboundForMs = grace))
        assertEquals(A11yStatus.DROPPED, status(unboundForMs = grace * 10))
        // Seen for the first time: not yet.
        assertEquals(A11yStatus.CONNECTING, status(unboundForMs = null))
    }

    /** On in the settings counts even when it was never seen connected (it was switched on, then crashed at once). */
    @Test fun droppedDoesNotNeedWasOn() {
        assertEquals(A11yStatus.DROPPED, status(wasOn = false, unboundForMs = grace))
    }

    /** Stopping the app by force takes it out of the settings (AccessibilityManagerService, onHandleForceStop). */
    @Test fun wasOnNowOffIsTurnedOff() {
        assertEquals(A11yStatus.TURNED_OFF, status(enabled = false, wasOn = true))
    }

    @Test fun neverOnOrCopyOnlyNeedsNothing() {
        assertEquals(A11yStatus.NOT_NEEDED, status(enabled = false, wasOn = false))
    }

    @Test fun keyOffNeedsNothing() {
        assertEquals(A11yStatus.NOT_NEEDED, status(keyOn = false, unboundForMs = grace * 2))
        assertEquals(A11yStatus.NOT_NEEDED, status(keyOn = false, enabled = false, wasOn = true))
    }

    @Test fun onlyDroppedAndTurnedOffAskForTheTap() {
        val asks = A11yStatus.entries.filter { A11yHealth.needsFix(it) }.toSet()
        assertEquals(setOf(A11yStatus.DROPPED, A11yStatus.TURNED_OFF), asks)
    }

    // --- how long it has been missing ---

    @Test fun unboundSinceStartsKeepsAndClears() {
        assertEquals(1_000L, A11yHealth.unboundSince(null, enabled = true, bound = false, now = 1_000))
        assertEquals(1_000L, A11yHealth.unboundSince(1_000, enabled = true, bound = false, now = 50_000))
        assertNull(A11yHealth.unboundSince(1_000, enabled = true, bound = true, now = 50_000))
        assertNull(A11yHealth.unboundSince(1_000, enabled = false, bound = false, now = 50_000))
    }

    @Test fun clockSetBackStartsAgain() {
        assertEquals(5_000L, A11yHealth.unboundSince(90_000, enabled = true, bound = false, now = 5_000))
    }

    // --- what is noted ---

    @Test fun changesAreNotedOnce() {
        assertEquals(A11yLog.Kind.DROPPED, A11yHealth.note(A11yStatus.CONNECTING, A11yStatus.DROPPED))
        assertEquals(A11yLog.Kind.DROPPED, A11yHealth.note(null, A11yStatus.DROPPED))
        assertEquals(A11yLog.Kind.TURNED_OFF, A11yHealth.note(A11yStatus.FINE, A11yStatus.TURNED_OFF))
        assertEquals(A11yLog.Kind.BACK, A11yHealth.note(A11yStatus.DROPPED, A11yStatus.FINE))
        assertEquals(A11yLog.Kind.BACK, A11yHealth.note(A11yStatus.TURNED_OFF, A11yStatus.FINE))
        assertNull(A11yHealth.note(A11yStatus.DROPPED, A11yStatus.DROPPED))
        // A restart within the grace is no drop, and no way back either.
        assertNull(A11yHealth.note(A11yStatus.FINE, A11yStatus.CONNECTING))
        assertNull(A11yHealth.note(A11yStatus.CONNECTING, A11yStatus.FINE))
        assertNull(A11yHealth.note(A11yStatus.NOT_NEEDED, A11yStatus.FINE))
    }

    // --- which notification says it ---

    @Test fun promptNotificationOnlyWhenTheKeysOwnIsNotUp() {
        assertTrue(A11yHealth.promptNotification(A11yStatus.DROPPED, keyNotificationShowing = false))
        assertTrue(A11yHealth.promptNotification(A11yStatus.TURNED_OFF, keyNotificationShowing = false))
        assertFalse(A11yHealth.promptNotification(A11yStatus.DROPPED, keyNotificationShowing = true))
        assertFalse(A11yHealth.promptNotification(A11yStatus.CONNECTING, keyNotificationShowing = false))
        assertFalse(A11yHealth.promptNotification(A11yStatus.FINE, keyNotificationShowing = false))
    }

    @Test fun keyNotificationLeadsWithTheFix() {
        assertEquals(KeyNotice.Line.FIX, KeyNotice.line(hidden = false, follows = false, a11y = A11yStatus.DROPPED))
        assertEquals(KeyNotice.Line.FIX, KeyNotice.line(hidden = true, follows = false, a11y = A11yStatus.TURNED_OFF))
        assertEquals(KeyNotice.Line.HIDDEN, KeyNotice.line(hidden = true, follows = false, a11y = A11yStatus.CONNECTING))
        assertEquals(KeyNotice.Line.TYPING, KeyNotice.line(hidden = false, follows = true, a11y = A11yStatus.FINE))
        // Back to the old line once it is connected again, or never was (copy only).
        assertEquals(KeyNotice.Line.ALWAYS, KeyNotice.line(hidden = false, follows = false, a11y = A11yStatus.NOT_NEEDED))
    }

    // --- the drop log ---

    @Test fun logLinesGoAndComeBack() {
        val e = A11yLog.Entry(1_764_300_000_000, A11yLog.Kind.EXIT, "dictation:low-memory")
        assertEquals(e, A11yLog.parse(A11yLog.line(e)))
        val plain = A11yLog.Entry(5, A11yLog.Kind.DROPPED)
        assertEquals(plain, A11yLog.parse(A11yLog.line(plain)))
    }

    /** Only the reason's word is kept: no spaces, tabs, line breaks or anything long. */
    @Test fun logDetailKeepsOnlyAWord() {
        val line = A11yLog.line(A11yLog.Entry(1, A11yLog.Kind.EXIT, "engine:crash\tsome text\nmore " + "x".repeat(100)))
        assertFalse(line.contains('\n'))
        assertEquals(3, line.split('\t').size)
        val detail = A11yLog.parse(line)!!.detail
        assertTrue(detail.startsWith("engine:crash"))
        assertTrue(detail.length <= 40)
        assertFalse(detail.contains(' '))
    }

    @Test fun logKeepsTheNewestAndDropsJunk() {
        var lines = listOf("not a line", "12\tNOPE\t", "")
        for (i in 1..(A11yLog.KEEP + 5)) lines = A11yLog.appended(lines, A11yLog.Entry(i.toLong(), A11yLog.Kind.DROPPED))
        assertEquals(A11yLog.KEEP, lines.size)
        assertEquals(6L, A11yLog.parse(lines.first())!!.at)
        assertEquals((A11yLog.KEEP + 5).toLong(), A11yLog.parse(lines.last())!!.at)
    }

    @Test fun parseRefusesBrokenLines() {
        assertNull(A11yLog.parse(""))
        assertNull(A11yLog.parse("abc\tDROPPED"))
        assertNull(A11yLog.parse("1\tUNKNOWN\tx"))
        assertNull(A11yLog.parse("1\tDROPPED\tx\ty"))
    }

    // --- which process ends are noted ---

    @Test fun anyEndOfTheKeysProcessIsNoted() {
        val p = "io.github.lokimdj.seogi:dictation"
        assertTrue(ExitNote.worth(p, ApplicationExitInfo.REASON_LOW_MEMORY))
        assertTrue(ExitNote.worth(p, ApplicationExitInfo.REASON_CRASH_NATIVE))
        assertTrue(ExitNote.worth(p, ApplicationExitInfo.REASON_USER_REQUESTED))
        assertTrue(ExitNote.worth(p, ApplicationExitInfo.REASON_OTHER))
        assertFalse(ExitNote.worth(p, ApplicationExitInfo.REASON_PACKAGE_UPDATED))
        assertFalse(ExitNote.worth(p, ApplicationExitInfo.REASON_EXIT_SELF))
    }

    /** The engine is let go when idle, so only its failures are noted. */
    @Test fun onlyTheEnginesFailuresAreNoted() {
        val p = "io.github.lokimdj.seogi:engine"
        assertTrue(ExitNote.worth(p, ApplicationExitInfo.REASON_CRASH_NATIVE))
        assertTrue(ExitNote.worth(p, ApplicationExitInfo.REASON_CRASH))
        assertTrue(ExitNote.worth(p, ApplicationExitInfo.REASON_ANR))
        assertFalse(ExitNote.worth(p, ApplicationExitInfo.REASON_LOW_MEMORY))
        assertFalse(ExitNote.worth(p, ApplicationExitInfo.REASON_OTHER))
    }

    @Test fun theMainProcessIsNotNoted() {
        assertFalse(ExitNote.worth("io.github.lokimdj.seogi", ApplicationExitInfo.REASON_CRASH))
    }

    @Test fun detailsNameTheProcessAndTheReason() {
        assertEquals("dictation:low-memory", ExitNote.detail("pkg:dictation", ApplicationExitInfo.REASON_LOW_MEMORY))
        assertEquals("engine:native-crash", ExitNote.detail("pkg:engine", ApplicationExitInfo.REASON_CRASH_NATIVE))
        assertEquals("dictation:force-stop", ExitNote.detail("pkg:dictation", ApplicationExitInfo.REASON_USER_REQUESTED))
        assertEquals("dictation:other", ExitNote.detail("pkg:dictation", 9_999))
    }

    @Test fun freshExitsAreNewerAndInOrder() {
        val exits = listOf(
            Triple(300L, "p:dictation", ApplicationExitInfo.REASON_LOW_MEMORY),
            Triple(100L, "p:dictation", ApplicationExitInfo.REASON_CRASH),
            Triple(200L, "p:engine", ApplicationExitInfo.REASON_LOW_MEMORY),
            Triple(250L, "p:engine", ApplicationExitInfo.REASON_CRASH_NATIVE),
        )
        val fresh = ExitNote.fresh(exits, seenUpTo = 100)
        assertEquals(listOf(250L, 300L), fresh.map { it.first })
    }
}
