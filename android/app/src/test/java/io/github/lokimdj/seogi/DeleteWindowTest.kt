package io.github.lokimdj.seogi

import io.github.lokimdj.seogi.upload.DeleteWindow
import io.github.lokimdj.seogi.upload.PendingDelete
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class DeleteWindowTest {
    private class Log {
        val hidden = mutableSetOf<String>()
        val committed = mutableListOf<PendingDelete>()
    }

    private fun TestScope.window(log: Log) = DeleteWindow(
        scope = this,
        onHide = { log.hidden += it },
        onShow = { log.hidden -= it },
        commit = { log.committed += it },
    )

    /** Undo inside the window: nothing is deleted, not even later. */
    @Test fun undoInTimeDeletesNothing() = runTest {
        val log = Log()
        val w = window(log)
        w.start(recording("a"), trashInDrive = true)
        assertEquals(setOf("a"), log.hidden)
        advanceTimeBy(DeleteWindow.WINDOW_MS - 1)
        assertTrue(w.undo())
        advanceTimeBy(60_000)
        runCurrent()
        assertEquals(emptyList<PendingDelete>(), log.committed)
        assertEquals(emptySet<String>(), log.hidden)
        assertNull(w.pending.value)
    }

    @Test fun theWindowIsEightSeconds() = runTest {
        val log = Log()
        val w = window(log)
        w.start(recording("a"), trashInDrive = true)
        advanceTimeBy(7_999)
        runCurrent()
        assertEquals(0, log.committed.size)
        advanceTimeBy(2)
        runCurrent()
        assertEquals(listOf("a"), log.committed.map { it.recording.id })
        assertEquals(true, log.committed[0].trashInDrive)
    }

    @Test fun undoAfterTheWindowIsTooLate() = runTest {
        val log = Log()
        val w = window(log)
        w.start(recording("a"), trashInDrive = false)
        advanceTimeBy(DeleteWindow.WINDOW_MS + 1)
        runCurrent()
        assertEquals(false, w.undo())
        assertEquals(1, log.committed.size)
    }

    /** Leaving the app, or a second delete, commits the first right away, once. */
    @Test fun leavingOrASecondDeleteCommitsOnce() = runTest {
        val log = Log()
        val w = window(log)
        w.start(recording("a"), trashInDrive = true)
        w.start(recording("b"), trashInDrive = true)
        runCurrent()
        assertEquals(listOf("a"), log.committed.map { it.recording.id })
        w.commitNow()
        advanceTimeBy(60_000)
        runCurrent()
        assertEquals(listOf("a", "b"), log.committed.map { it.recording.id })
    }
}
