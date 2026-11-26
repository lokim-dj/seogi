package io.github.lokimdj.seogi

import io.github.lokimdj.seogi.data.Recording
import io.github.lokimdj.seogi.data.RecordingStore
import io.github.lokimdj.seogi.data.UploadError
import io.github.lokimdj.seogi.data.UploadState
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import java.io.File

fun recording(id: String, startedAt: Long = 1000, upload: UploadState = UploadState.WAITING, error: UploadError? = null) = Recording(
    id = id, title = "회의 $id", startedAt = startedAt, zone = "Asia/Seoul", durationMs = 60_000,
    fileName = "$id.m4a", mimeType = "audio/mp4", mediaUri = "content://x/$id", sizeBytes = 10, sha256 = "00",
    upload = upload, error = error,
)

class RecordingStoreTest {
    @get:Rule val tmp = TemporaryFolder()

    @Test fun keepsAcrossRestarts() = runTest {
        val f = File(tmp.root, "r.json")
        val a = RecordingStore(f)
        a.upsert(recording("a", 1))
        a.upsert(recording("b", 2))
        a.update("a") { it.copy(upload = UploadState.UPLOADED) }
        val b = RecordingStore(f)
        assertEquals(listOf("b", "a"), b.all.value.map { it.id })
        assertEquals(UploadState.UPLOADED, b.get("a")!!.upload)
    }

    @Test fun brokenFileIsMovedAsideNotLost() {
        val f = File(tmp.root, "r.json").apply { writeText("{not json") }
        val s = RecordingStore(f)
        assertTrue(s.all.value.isEmpty())
        assertFalse(f.exists())
        assertEquals(1, tmp.root.listFiles()!!.count { it.name.startsWith("r.json.broken-") })
    }

    @Test fun whatNeedsUpload() {
        assertTrue(recording("a").needsUpload)
        assertTrue(recording("a", upload = UploadState.FAILED, error = UploadError.OFFLINE).needsUpload)
        assertFalse(recording("a", upload = UploadState.FAILED, error = UploadError.NEED_LOGIN).needsUpload)
        assertFalse(recording("a", upload = UploadState.FAILED, error = UploadError.MISSING_ON_PHONE).needsUpload)
        assertFalse(recording("a", upload = UploadState.UPLOADED).needsUpload)
    }
}
