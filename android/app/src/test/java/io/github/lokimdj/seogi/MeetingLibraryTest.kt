package io.github.lokimdj.seogi

import io.github.lokimdj.seogi.data.DriveMeeting
import io.github.lokimdj.seogi.data.MeetingDoc
import io.github.lokimdj.seogi.data.MeetingLibrary
import io.github.lokimdj.seogi.data.MeetingList
import io.github.lokimdj.seogi.data.Recording
import io.github.lokimdj.seogi.upload.DriveFolder
import io.github.lokimdj.seogi.upload.DriveItem
import io.github.lokimdj.seogi.upload.DriveReader
import io.github.lokimdj.seogi.upload.TokenResult
import io.github.lokimdj.seogi.upload.Uploader
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import java.io.File
import java.io.IOException

class MeetingLibraryTest {
    @get:Rule val tmp = TemporaryFolder()

    /** The shared folder in memory: folders in the root, files in them. */
    private class FakeReader : DriveReader {
        val folders = mutableListOf<DriveFolder>()
        val files = mutableListOf<DriveItem>()
        val text = mutableMapOf<String, String>()
        val bytes = mutableMapOf<String, ByteArray>()
        val reads = mutableListOf<String>()
        var downloads = 0
        var offline = false
        var made = 0
        var usable = true

        override fun ensureFolder(token: String): String { made++; return "ROOT" }
        override fun folderUsable(token: String, folderId: String) = usable && folderId == "ROOT"
        override fun folders(token: String, root: String): List<DriveFolder> {
            if (offline) throw IOException("offline")
            return if (root == "ROOT") folders.toList() else emptyList()
        }
        override fun filesIn(token: String, parents: List<String>) = files.filter { it.parent in parents }
        override fun readText(token: String, fileId: String): String { reads += fileId; return text.getValue(fileId) }
        override fun download(token: String, fileId: String, to: File, onProgress: (Long, Long) -> Unit) {
            downloads++
            val b = bytes.getValue(fileId)
            to.writeBytes(b)
            onProgress(b.size.toLong(), b.size.toLong())
        }

        fun meeting(id: String, title: String, started: Long, origin: String = "recorded", audio: String? = "녹음.wav", rec: String? = null, version: Long = 1, tasks: String = "") {
            val folder = "F-$id"
            folders += DriveFolder(folder, "$title 폴더", id)
            text["J-$id"] = """{"format":1,"id":"$id","title":"$title","started_at_ms":$started,"duration_ms":60000,
                "origin":"$origin","audio":{"file":"${audio ?: ""}","in_folder":${audio != null}},
                "tasks":[$tasks],"future_field":{"x":1}}"""
            files += DriveItem("J-$id", "회의.json", folder, 300, "application/json", version, null)
            files += DriveItem("T-$id", "전사.txt", folder, 100, "text/plain", 1, null)
            if (audio != null) files += DriveItem("A-$id", audio, folder, 4, "audio/wav", 1, rec)
            bytes["A-$id"] = byteArrayOf(1, 2, 3, 4)
        }
    }

    private class Folder(override var folderId: String? = null) : Uploader.FolderCache

    private fun library(reader: FakeReader, folder: Folder = Folder(), signedIn: Boolean = true) = MeetingLibrary(
        dir = File(tmp.root, "meetings"),
        reader = reader,
        tokens = { if (signedIn) TokenResult.Ok("t") else TokenResult.NeedLogin },
        folder = folder,
    )

    private val task = """{"id":"a1","text":"견적서 보내기","owner":"가","done":false,"sources":[{"ms":61000,"quote":"금요일까지"}]}"""

    /** Truth 2: the computer's meetings show on the phone, newest first, with their tasks counted. */
    @Test fun listsEveryMeetingFolderNewestFirst() = runBlocking {
        val r = FakeReader()
        r.meeting("m1", "견적 회의", 1000, tasks = task)
        r.meeting("m2", "주간 회의", 2000)
        val lib = library(r)
        assertEquals(MeetingLibrary.Outcome.DONE, lib.refresh())
        assertEquals(listOf("m2", "m1"), lib.all.value.map { it.id })
        val m1 = lib.get("m1")!!
        assertEquals(1, m1.openTasks)
        assertEquals("A-m1", m1.audioFileId)
        assertEquals("견적서 보내기", lib.doc("m1")!!.tasks.single().text)
        assertEquals(61000L, lib.doc("m1")!!.tasks.single().sources.single().ms)
    }

    /** A folder still being written (no 회의.json yet) and an ordinary folder are not meetings. */
    @Test fun aFolderWithoutItsJsonIsNotReadyYet() = runBlocking {
        val r = FakeReader()
        r.meeting("m1", "견적 회의", 1000)
        r.files.removeAll { it.name == "회의.json" }
        r.folders += DriveFolder("X", "다른 폴더", null)
        val lib = library(r)
        lib.refresh()
        assertTrue(lib.all.value.isEmpty())
    }

    /** Only a new or changed 회의.json is read again; the rest comes from the phone. */
    @Test fun readsOnlyWhatChanged() = runBlocking {
        val r = FakeReader()
        r.meeting("m1", "견적 회의", 1000)
        val lib = library(r)
        lib.refresh()
        lib.refresh()
        assertEquals(listOf("J-m1"), r.reads)
        r.files.replaceAll { if (it.id == "J-m1") it.copy(version = 2) else it }
        r.text["J-m1"] = r.text["J-m1"]!!.replace("견적 회의", "견적 회의 2")
        lib.refresh()
        assertEquals(2, r.reads.size)
        assertEquals("견적 회의 2", lib.get("m1")!!.title)
    }

    /** Offline: what was read before stays, and the minutes still open. */
    @Test fun offlineKeepsWhatWasRead() = runBlocking {
        val r = FakeReader()
        r.meeting("m1", "견적 회의", 1000)
        library(r).refresh()
        r.offline = true
        val again = library(r, Folder("ROOT"))
        assertEquals(MeetingLibrary.Outcome.OFFLINE, again.refresh())
        assertEquals(listOf("m1"), again.all.value.map { it.id })
        assertNotNull(again.doc("m1"))
    }

    /** A meeting taken out of Drive (its folder in the trash) leaves the phone's list too. */
    @Test fun aMeetingTakenOutLeavesTheList() = runBlocking {
        val r = FakeReader()
        r.meeting("m1", "견적 회의", 1000)
        r.meeting("m2", "주간 회의", 2000)
        val lib = library(r)
        lib.refresh()
        r.folders.removeAll { it.meetingId == "m1" }
        lib.refresh()
        assertEquals(listOf("m2"), lib.all.value.map { it.id })
        assertNull(lib.doc("m1"))
    }

    @Test fun signedOutSaysSo() = runBlocking {
        assertEquals(MeetingLibrary.Outcome.NEED_LOGIN, library(FakeReader(), signedIn = false).refresh())
    }

    /** A saved folder that went to the trash: the marked one is found (or made) again. */
    @Test fun aTrashedFolderIsFoundAgain() = runBlocking {
        val r = FakeReader()
        r.meeting("m1", "견적 회의", 1000)
        val folder = Folder("OLD")
        library(r, folder).refresh()
        assertEquals("ROOT", folder.folderId)
        assertEquals(1, r.made)
    }

    /** Truth 4: the recording comes down only when asked for, once, and the Drive file is only read. */
    @Test fun theRecordingIsDownloadedOnlyWhenAskedOnce() = runBlocking {
        val r = FakeReader()
        r.meeting("m1", "견적 회의", 1000)
        val lib = library(r)
        lib.refresh()
        assertEquals(0, r.downloads)
        val cache = tmp.newFolder("cache")
        var progress = 0f
        val f = lib.audio("m1", cache) { progress = it }!!
        assertEquals(4L, f.length())
        assertEquals(1f, progress)
        lib.audio("m1", cache) {}
        assertEquals(1, r.downloads)
    }

    /** A piece left longer than the recording (two downloads mixed) is thrown away, not played. */
    @Test fun aPieceTooLongIsStartedOver() = runBlocking {
        val r = FakeReader()
        r.meeting("m1", "견적 회의", 1000)
        val lib = library(r)
        lib.refresh()
        val cache = tmp.newFolder("cache")
        File(cache, "meeting-audio").mkdirs()
        File(cache, "meeting-audio/A-m1.wav.part").writeBytes(ByteArray(10))
        assertEquals(4L, lib.audio("m1", cache) {}!!.length())
    }

    /** Drive answers 403 also for going too fast: that is no sign-in matter. */
    @Test fun aRateLimitIsNotASignIn() {
        assertTrue(MeetingLibrary.needsLogin(io.github.lokimdj.seogi.upload.DriveHttpException(401, "")))
        assertTrue(MeetingLibrary.needsLogin(io.github.lokimdj.seogi.upload.DriveHttpException(403, "insufficientPermissions")))
        assertFalse(MeetingLibrary.needsLogin(io.github.lokimdj.seogi.upload.DriveHttpException(403, "userRateLimitExceeded")))
    }

    @Test fun aMeetingWithoutItsRecordingInDriveHasNothingToDownload() = runBlocking {
        val r = FakeReader()
        r.meeting("m1", "견적 회의", 1000, audio = null)
        val lib = library(r)
        lib.refresh()
        assertNull(lib.get("m1")!!.audioFileId)
        assertNull(lib.audio("m1", tmp.newFolder("cache")) {})
    }

    /** Truth 3: a phone recording whose minutes came back shows as one line, with them. */
    @Test fun aPhoneRecordingAndItsMeetingAreOneLine() = runBlocking {
        val r = FakeReader()
        r.meeting("m1", "주간 회의", 5000, origin = "phone", audio = "2026-11-26 1842 주간 회의.m4a", rec = "rec-1")
        r.meeting("m2", "견적 회의", 1000)
        val lib = library(r)
        lib.refresh()
        assertEquals("rec-1", lib.get("m1")!!.phoneRecordingId)
        val list = MeetingList.merge(listOf(rec("rec-1", 5000), rec("rec-2", 9000)), lib.all.value)
        assertEquals(listOf("rec-2", "rec-1", "m:m2"), list.map { it.key })
        assertEquals("주간 회의", list[1].title)
        assertNotNull(list[1].meeting)
        assertNull(list[0].meeting)
    }

    /** Files uploaded before they carried the recording's id are matched by name. */
    @Test fun anOlderPhoneRecordingIsMatchedByName() {
        val m = DriveMeeting(
            id = "m1", folderId = "F", folderName = "f", jsonFileId = "J", title = "주간", startedAtMs = 1, durationMs = 1,
            origin = "phone", audioName = "a.m4a",
        )
        val list = MeetingList.merge(listOf(rec("rec-1", 1, "a.m4a")), listOf(m))
        assertEquals(1, list.size)
        assertEquals("m1", list.single().meeting?.id)
    }

    @Test fun unknownFieldsAndMissingOnesAreFine() {
        val doc = MeetingDoc.parse("""{"id":"x","new":"later","lines":[{"start_ms":5,"text":"안녕"}]}""")!!
        assertEquals(5L, doc.lines.single().startMs)
        assertNull(doc.lines.single().speaker)
        assertNull(doc.speakerName(2))
        assertNull(MeetingDoc.parse("not json"))
        assertNull(MeetingDoc.parse("""{"title":"no id"}"""))
        assertFalse(doc.audio.inFolder)
    }

    private fun rec(id: String, started: Long, file: String = "$id.m4a") = Recording(
        id = id, title = "폰 녹음 $id", startedAt = started, zone = "Asia/Seoul", durationMs = 1000,
        fileName = file, mimeType = "audio/mp4", sizeBytes = 10, sha256 = "",
    )
}
