package io.github.lokimdj.seogi

import io.github.lokimdj.seogi.record.SessionDir
import io.github.lokimdj.seogi.record.SessionState
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder

class SessionTest {
    @get:Rule val tmp = TemporaryFolder()

    @Test fun stateRoundTripsWithMarks() {
        val d = SessionDir(tmp.root)
        val s = SessionState("rec-1", 1, "Asia/Seoul", "주간 회의")
        d.write(s)
        d.write(s.copy(marksMs = listOf(760_000, 1_865_000)))
        assertEquals(listOf(760_000L, 1_865_000L), d.read("rec-1")!!.marksMs)
    }

    /** Review focus 1: a recording cut off by a crash is found on the next start. */
    @Test fun leftoversAreFoundExceptTheRunningOne() {
        val d = SessionDir(tmp.root)
        d.write(SessionState("rec-100", 100, "Asia/Seoul", "점검"))
        d.audio("rec-100").writeBytes(ByteArray(10))
        d.audio("rec-200").writeBytes(ByteArray(10)) // state lost: start from the id
        d.audio("rec-300").writeBytes(ByteArray(10)) // running now
        val found = d.leftovers(activeId = "rec-300", zone = "Asia/Seoul")
        assertEquals(listOf("rec-100", "rec-200"), found.map { it.id })
        assertEquals("점검", found[0].eventTitle)
        assertEquals(200L, found[1].startedAt)
    }

    @Test fun clearRemovesStreamAndState() {
        val d = SessionDir(tmp.root)
        d.write(SessionState("rec-1", 1, "Asia/Seoul"))
        d.audio("rec-1").writeBytes(ByteArray(1))
        d.clear("rec-1")
        assertNull(d.read("rec-1"))
        assertEquals(emptyList<SessionState>(), d.leftovers(null, "Asia/Seoul"))
    }
}
