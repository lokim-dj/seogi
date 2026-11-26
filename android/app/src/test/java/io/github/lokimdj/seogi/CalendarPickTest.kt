package io.github.lokimdj.seogi

import io.github.lokimdj.seogi.record.CalendarEvent
import io.github.lokimdj.seogi.record.CalendarPick
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class CalendarPickTest {
    private val min = 60_000L
    private val now = 1_000 * min

    @Test fun underWayNamesTheRecording() {
        val e = CalendarEvent("주간 회의", now - 5 * min, now + 30 * min)
        assertEquals(e, CalendarPick.current(listOf(e), now))
    }

    @Test fun startingWithinTenMinutesCounts() {
        val soon = CalendarEvent("상담", now + 9 * min, now + 60 * min)
        val later = CalendarEvent("나중", now + 11 * min, now + 60 * min)
        assertEquals(soon, CalendarPick.current(listOf(later, soon), now))
        assertNull(CalendarPick.current(listOf(later), now))
    }

    @Test fun overlappingPicksTheClosestStart() {
        val long = CalendarEvent("하루 워크숍", now - 120 * min, now + 120 * min)
        val meeting = CalendarEvent("점검", now - 2 * min, now + 30 * min)
        assertEquals(meeting, CalendarPick.current(listOf(long, meeting), now))
    }

    @Test fun endedOrUntitledDoNotCount() {
        assertNull(CalendarPick.current(listOf(CalendarEvent("끝남", now - 60 * min, now)), now))
        assertNull(CalendarPick.current(listOf(CalendarEvent(" ", now, now + min)), now))
    }

    @Test fun homeShowsTheNextWhenNothingIsOn() {
        val next = CalendarEvent("오후 회의", now + 90 * min, now + 120 * min)
        val after = CalendarEvent("저녁", now + 300 * min, now + 360 * min)
        assertEquals(next, CalendarPick.next(listOf(after, next), now))
    }
}
