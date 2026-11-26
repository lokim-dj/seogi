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

    private class TrashDrive(val fail: Boolean = false) : Drive {
        val trashed = mutableListOf<String>()
        override fun trash(token: String, fileId: String) {
            if (fail) throw DriveHttpException(500, "busy")
            trashed += fileId
        }
        override fun ensureFolder(token: String) = error("not used")
        override fun folderUsable(token: String, folderId: String) = error("not used")
        override fun uploadSmall(token: String, folderId: String, name: String, mime: String, bytes: ByteArray) = error("not used")
        override fun startSession(token: String, folderId: String, name: String, mime: String, size: Long) = error("not used")
        override fun status(token: String, session: String, size: Long): UploadStatus = error("not used")
        override fun putChunk(token: String, session: String, bytes: ByteArray, from: Long, size: Long): UploadStatus = error("not used")
        override fun email(token: String) = null
    }

    private val uploaded = recording("a", upload = UploadState.UPLOADED).copy(driveFileId = "AUDIO", sidecarFileId = "JSON")

    private fun deleter(store: RecordingStore, drive: Drive, phone: Deleter.PhoneResult = Deleter.PhoneResult.DELETED) =
        Deleter(store, drive, { TokenResult.Ok("t") }, { phone })

    @Test fun trashesTheAudioAndItsJsonThenRemovesIt() = runTest {
        val s = RecordingStore(File(tmp.root, "r.json")).apply { upsert(uploaded) }
        val d = TrashDrive()
        val r = deleter(s, d).delete(uploaded, trashInDrive = true)
        assertEquals(listOf("AUDIO", "JSON"), d.trashed)
        assertEquals(Deleter.DriveResult.TRASHED, r.drive)
        assertNull(s.get("a"))
    }

    @Test fun leavesDriveAloneWhenUnticked() = runTest {
        val s = RecordingStore(File(tmp.root, "r.json")).apply { upsert(uploaded) }
        val d = TrashDrive()
        assertEquals(Deleter.DriveResult.LEFT_IN_DRIVE, deleter(s, d).delete(uploaded, trashInDrive = false).drive)
        assertEquals(emptyList<String>(), d.trashed)
        assertNull(s.get("a"))
    }

    @Test fun notYetUploadedTouchesNothingInDrive() = runTest {
        val waiting = recording("b")
        val s = RecordingStore(File(tmp.root, "r.json")).apply { upsert(waiting) }
        val d = TrashDrive()
        assertEquals(Deleter.DriveResult.NOT_UPLOADED, deleter(s, d).delete(waiting, trashInDrive = true).drive)
        assertEquals(emptyList<String>(), d.trashed)
    }

    @Test fun aPhoneFileThatCannotBeDeletedStaysListed() = runTest {
        val s = RecordingStore(File(tmp.root, "r.json")).apply { upsert(uploaded) }
        val r = deleter(s, TrashDrive(), Deleter.PhoneResult.FAILED).delete(uploaded, trashInDrive = false)
        assertEquals(false, r.phoneDeleted)
        assertNotNull(s.get("a"))
    }

    @Test fun driveTroubleIsReportedNotHidden() = runTest {
        val s = RecordingStore(File(tmp.root, "r.json")).apply { upsert(uploaded) }
        val r = deleter(s, TrashDrive(fail = true)).delete(uploaded, trashInDrive = true)
        assertEquals(Deleter.DriveResult.FAILED, r.drive)
        assertNull(s.get("a"))
    }
}
