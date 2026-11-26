package io.github.lokimdj.seogi

import io.github.lokimdj.seogi.data.PhoneFile
import io.github.lokimdj.seogi.data.Reconcile
import org.junit.Assert.assertEquals
import org.junit.Test
import java.time.ZoneId
import java.time.ZonedDateTime

/** Fix 4: a list restored from its backup is matched with the files on the phone. */
class ReconcileTest {
    private val zone = ZoneId.of("Asia/Seoul")

    @Test fun aFileTheBackupMissedComesBackAndAGoneFileGoes() {
        val known = recording("a").copy(mediaUri = "content://media/1")
        val gone = recording("b").copy(mediaUri = "content://media/2")
        val saving = recording("c").copy(mediaUri = null)
        val files = listOf(
            PhoneFile("content://media/1", "a.m4a", "audio/mp4", 10, 1000, 0),
            PhoneFile("content://media/3", "2026-11-26 1405 주간 회의.m4a", "audio/mp4", 99, 60_000, 0),
        )
        val merged = Reconcile.merge(listOf(known, gone, saving), files, zone)
        assertEquals(setOf("content://media/1", "content://media/3", null), merged.map { it.mediaUri }.toSet())
        val back = merged.first { it.mediaUri == "content://media/3" }
        assertEquals("주간 회의", back.title)
        assertEquals(ZonedDateTime.of(2026, 11, 26, 14, 5, 0, 0, zone).toInstant().toEpochMilli(), back.startedAt)
        assertEquals(true, back.recovered)
        assertEquals(true, back.needsUpload)
    }
}
