package io.github.lokimdj.seogi

import io.github.lokimdj.seogi.data.MeetingLook
import io.github.lokimdj.seogi.ui.Pull
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class MeetingLookTest {
    @Test fun aPullReadsAtOnceEvenWithinTheMinute() {
        val look = MeetingLook()
        assertTrue(look.start(signedIn = true, nowMs = 1_000, now = false))
        look.done()
        assertFalse(look.start(signedIn = true, nowMs = 2_000, now = false))
        assertTrue(look.start(signedIn = true, nowMs = 3_000, now = true))
        assertTrue(look.reading.value)
    }

    @Test fun neverTwoReadsAtOnce() {
        val look = MeetingLook()
        assertTrue(look.start(signedIn = true, nowMs = 0, now = true))
        assertFalse(look.start(signedIn = true, nowMs = 10, now = true))
        assertFalse(look.start(signedIn = true, nowMs = 120_000, now = false))
        assertTrue(look.reading.value)
        look.done()
        assertFalse(look.reading.value)
        assertTrue(look.start(signedIn = true, nowMs = 20, now = true))
    }

    @Test fun aPullDuringAReadIsDropped() {
        val look = MeetingLook()
        assertTrue(look.start(signedIn = true, nowMs = 0, now = false))
        assertFalse(look.start(signedIn = true, nowMs = 10, now = true, pull = true))
        assertFalse(look.done())
    }

    @Test fun aReadAfterSignInWaitsForTheRunningOne() {
        val look = MeetingLook()
        assertTrue(look.start(signedIn = true, nowMs = 0, now = false))
        assertFalse(look.start(signedIn = true, nowMs = 10, now = true))
        assertTrue(look.done())
        assertFalse(look.done())
    }

    @Test fun onlyAPulledReadShowsTheRing() {
        val look = MeetingLook()
        assertTrue(look.start(signedIn = true, nowMs = 0, now = false))
        assertFalse(look.pulled.value)
        look.done()
        assertTrue(look.start(signedIn = true, nowMs = 10, now = true, pull = true))
        assertTrue(look.pulled.value)
        look.done()
        assertFalse(look.pulled.value)
    }

    @Test fun notSignedInEndsAtOnce() {
        val look = MeetingLook()
        assertFalse(look.start(signedIn = false, nowMs = 0, now = true))
        assertFalse(look.reading.value)
        // A pull before sign-in does not use up the first look.
        assertTrue(look.start(signedIn = true, nowMs = 1, now = false))
    }

    @Test fun onItsOwnOnceAMinute() {
        val look = MeetingLook()
        assertTrue(look.start(signedIn = true, nowMs = 0, now = false))
        look.done()
        assertFalse(look.start(signedIn = true, nowMs = 59_999, now = false))
        assertTrue(look.start(signedIn = true, nowMs = 60_000, now = false))
    }

    @Test fun thePullFollowsTheFingerAndSlowsDown() {
        val max = 200f
        var d = 0f
        repeat(100) { d = Pull.drag(d, 10f, max) }
        assertTrue(d <= max)
        assertTrue(Pull.drag(0f, 10f, max) > Pull.drag(150f, 10f, max) - 150f)
        assertEquals(0f, Pull.drag(5f, -10f, max))
    }

    @Test fun onlyAFullPullReads() {
        assertFalse(Pull.reads(pulled = 79f, threshold = 80f))
        assertTrue(Pull.reads(pulled = 80f, threshold = 80f))
    }

    @Test fun theRingTurnsWithThePull() {
        assertEquals(0f, Pull.turn(0f, 80f))
        assertEquals(270f, Pull.turn(80f, 80f))
        assertEquals(270f, Pull.turn(200f, 80f))
    }
}
