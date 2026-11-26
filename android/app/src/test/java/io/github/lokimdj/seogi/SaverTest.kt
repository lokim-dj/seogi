package io.github.lokimdj.seogi

import io.github.lokimdj.seogi.data.RecordingStore
import io.github.lokimdj.seogi.record.Saver
import io.github.lokimdj.seogi.record.SessionDir
import io.github.lokimdj.seogi.record.SessionState
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import java.io.File
import java.io.IOException

/** Review 6 and the minor "saved twice": saving is loud when it fails and happens once. */
class SaverTest {
    @get:Rule val tmp = TemporaryFolder()

    private class Calls {
        val finalized = mutableListOf<String>()
        val discarded = mutableListOf<String>()
        val failures = mutableListOf<String>()
        val firstFailures = mutableListOf<String>()
        var enqueued = 0
    }

    private fun setup(fail: Boolean = false, crashAfterPending: Boolean = false): Triple<Saver, SessionDir, Pair<RecordingStore, Calls>> {
        val calls = Calls()
        val store = RecordingStore(File(tmp.root, "r.json"))
        val dir = SessionDir(File(tmp.root, "rec"))
        val saver = Saver(
            store = store,
            finalize = { _, state, _, onPending ->
                onPending("content://media/${calls.finalized.size + 1}")
                if (crashAfterPending) throw IllegalStateException("killed while writing")
                if (fail) throw IOException("disk full")
                calls.finalized += state.id
                recording(state.id).copy(mediaUri = "content://media/${calls.finalized.size}")
            },
            discardPublished = { calls.discarded += it },
            enqueue = { calls.enqueued++ },
            onFailure = { id, _, first -> calls.failures += id; if (first) calls.firstFailures += id },
            onSuccess = {},
        )
        return Triple(saver, dir, store to calls)
    }

    private fun SessionDir.stream(id: String) {
        write(SessionState(id, 1, "Asia/Seoul"))
        audio(id).writeBytes(ByteArray(100))
    }

    @Test fun aFailedSaveIsToldAndTheStreamStays() = runTest {
        val (saver, dir, sc) = setup(fail = true)
        dir.stream("rec-1")
        saver.save(dir, "rec-1", recovered = false, zone = "Asia/Seoul")
        assertEquals(listOf("rec-1"), sc.second.failures)
        assertTrue(dir.audio("rec-1").exists())
        assertEquals(emptyList<Any>(), sc.first.all.value)
    }

    @Test fun aSaveCutOffMidWriteIsRedoneOnceNotTwice() = runTest {
        val (crashing, dir, _) = setup(crashAfterPending = true)
        dir.stream("rec-1")
        crashing.save(dir, "rec-1", recovered = false, zone = "Asia/Seoul")
        assertEquals("content://media/1", dir.readPublishing("rec-1"))

        val (saver, _, sc) = setup()
        saver.save(dir, "rec-1", recovered = true, zone = "Asia/Seoul")
        assertEquals(listOf("content://media/1"), sc.second.discarded)
        assertEquals(listOf("rec-1"), sc.second.finalized)
        assertEquals(1, sc.first.all.value.size)
        assertEquals(false, dir.audio("rec-1").exists())
    }

    @Test fun finishedButNotListedIsListedWithoutSavingAgain() = runTest {
        val (saver, dir, sc) = setup()
        dir.stream("rec-1")
        dir.writeSaved(recording("rec-1").copy(mediaUri = "content://media/9"))
        saver.save(dir, "rec-1", recovered = true, zone = "Asia/Seoul")
        assertEquals(emptyList<String>(), sc.second.finalized)
        assertEquals("content://media/9", sc.first.get("rec-1")!!.mediaUri)
        assertEquals(1, sc.second.enqueued)
    }

    /** Fix 4: the same failed save tells the user once, not on every start. */
    @Test fun aFailedSaveIsToldOnce() = runTest {
        val (saver, dir, sc) = setup(fail = true)
        dir.stream("rec-1")
        saver.save(dir, "rec-1", recovered = false, zone = "Asia/Seoul")
        saver.save(dir, "rec-1", recovered = true, zone = "Asia/Seoul")
        assertEquals(listOf("rec-1", "rec-1"), sc.second.failures)
        assertEquals(listOf("rec-1"), sc.second.firstFailures)
    }
}
