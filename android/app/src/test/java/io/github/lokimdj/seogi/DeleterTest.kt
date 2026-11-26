package io.github.lokimdj.seogi

import io.github.lokimdj.seogi.data.RecordingStore
import io.github.lokimdj.seogi.data.UploadState
import io.github.lokimdj.seogi.upload.Deleter
import io.github.lokimdj.seogi.upload.Drive
import io.github.lokimdj.seogi.upload.DriveHttpException
import io.github.lokimdj.seogi.upload.TokenResult
import io.github.lokimdj.seogi.upload.UploadStatus
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import java.io.File

class DeleterTest {
    @get:Rule val tmp = TemporaryFolder()

    private class TrashDrive(val fail: Boolean = false, val sessionStatus: UploadStatus = UploadStatus.Expired) : Drive {
        val trashed = mutableListOf<String>()
        val cancelled = mutableListOf<String>()
        override fun trash(token: String, fileId: String): Boolean {
            if (fail) throw DriveHttpException(500, "busy")
            trashed += fileId
            return true
        }
        override fun cancelSession(session: String) { cancelled += session }
        override fun status(token: String, session: String, size: Long): UploadStatus = sessionStatus
        override fun ensureFolder(token: String) = error("not used")
        override fun folderUsable(token: String, folderId: String) = error("not used")
        override fun uploadSmall(token: String, folderId: String, name: String, mime: String, bytes: ByteArray) = error("not used")
        override fun startSession(token: String, folderId: String, name: String, mime: String, size: Long) = error("not used")
        override fun putChunk(token: String, session: String, bytes: ByteArray, from: Long, size: Long): UploadStatus = error("not used")
        override fun email(token: String) = null
    }

    private val uploaded = recording("a", upload = UploadState.UPLOADED)
        .copy(driveFileId = "AUDIO", sidecarFileId = "JSON", account = "me@example.com")

    private fun store() = RecordingStore(File(tmp.root, "r.json"))

    private fun deleter(
        store: RecordingStore,
        drive: Drive,
        phone: Deleter.PhoneResult = Deleter.PhoneResult.DELETED,
        account: String? = "me@example.com",
    ) = Deleter(store, drive, { TokenResult.Ok("t") }, { account }, { phone })

    @Test fun trashesTheAudioAndItsJsonThenRemovesIt() = runTest {
        val s = store().apply { upsert(uploaded) }
        val d = TrashDrive()
        val r = deleter(s, d).delete(uploaded, trashInDrive = true)
        assertEquals(listOf("AUDIO", "JSON"), d.trashed)
        assertEquals(Deleter.DriveResult.TRASHED, r.drive)
        assertNull(s.get("a"))
    }

    @Test fun leavesDriveAloneWhenUnticked() = runTest {
        val s = store().apply { upsert(uploaded) }
        val d = TrashDrive()
        assertEquals(Deleter.DriveResult.LEFT_IN_DRIVE, deleter(s, d).delete(uploaded, trashInDrive = false).drive)
        assertEquals(emptyList<String>(), d.trashed)
        assertNull(s.get("a"))
    }

    @Test fun notYetUploadedTouchesNothingInDrive() = runTest {
        val waiting = recording("b")
        val s = store().apply { upsert(waiting) }
        val d = TrashDrive()
        assertEquals(Deleter.DriveResult.NOT_UPLOADED, deleter(s, d).delete(waiting, trashInDrive = true).drive)
        assertEquals(emptyList<String>(), d.trashed)
    }

    /** Review 1: an upload under way that had not finished is given up, so no half file stays. */
    @Test fun halfDoneUploadIsGivenUp() = runTest {
        val r = recording("c").copy(upload = UploadState.UPLOADING, sidecarFileId = "JSON", sessionUri = "s1")
        val s = store().apply { upsert(r) }
        val d = TrashDrive(sessionStatus = UploadStatus.Incomplete(1024))
        assertEquals(Deleter.DriveResult.TRASHED, deleter(s, d).delete(r, trashInDrive = true).drive)
        assertEquals(listOf("s1"), d.cancelled)
        assertEquals(listOf("JSON"), d.trashed)
    }

    /** Review 1: it finished just as the user deleted it: that file goes to the trash too. */
    @Test fun justFinishedUploadIsTrashedToo() = runTest {
        val r = recording("c").copy(upload = UploadState.UPLOADING, sidecarFileId = "JSON", sessionUri = "s1")
        val s = store().apply { upsert(r) }
        val d = TrashDrive(sessionStatus = UploadStatus.Complete("AUDIO"))
        deleter(s, d).delete(r, trashInDrive = true)
        assertEquals(listOf("JSON", "AUDIO"), d.trashed)
        assertEquals(emptyList<String>(), d.cancelled)
    }

    /** Review 2: uploaded with another account: say so, do not pretend it was trashed. */
    @Test fun anotherAccountIsReportedNotTrashed() = runTest {
        val s = store().apply { upsert(uploaded) }
        val d = TrashDrive()
        val r = deleter(s, d, account = "other@example.com").delete(uploaded, trashInDrive = true)
        assertEquals(Deleter.DriveResult.OTHER_ACCOUNT, r.drive)
        assertEquals("me@example.com", r.otherAccount)
        assertEquals(emptyList<String>(), d.trashed)
        assertNull(s.get("a"))
    }

    @Test fun aPhoneFileThatCannotBeDeletedStaysListed() = runTest {
        val s = store().apply { upsert(uploaded) }
        val r = deleter(s, TrashDrive(), Deleter.PhoneResult.FAILED).delete(uploaded, trashInDrive = false)
        assertEquals(false, r.phoneDeleted)
        assertNotNull(s.get("a"))
    }

    @Test fun driveTroubleIsReportedNotHidden() = runTest {
        val s = store().apply { upsert(uploaded) }
        val r = deleter(s, TrashDrive(fail = true)).delete(uploaded, trashInDrive = true)
        assertEquals(Deleter.DriveResult.FAILED, r.drive)
        assertNull(s.get("a"))
    }

    /** Minor: the list cannot be written: the result says so instead of throwing. */
    @Test fun aListThatCannotBeWrittenIsReported() = runTest {
        val dir = File(tmp.root, "ro").apply { mkdirs() }
        val s = RecordingStore(File(dir, "r.json")).apply { upsert(uploaded) }
        dir.setWritable(false)
        try {
            val r = deleter(s, TrashDrive()).delete(uploaded, trashInDrive = false)
            assertEquals(true, r.phoneDeleted)
            assertEquals(false, r.listRemoved)
        } finally {
            dir.setWritable(true)
        }
    }
}
