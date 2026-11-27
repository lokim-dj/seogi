package io.github.lokimdj.seogi

import io.github.lokimdj.seogi.data.Edit
import io.github.lokimdj.seogi.data.Edits
import io.github.lokimdj.seogi.data.EditsFile
import io.github.lokimdj.seogi.data.MeetingDoc
import io.github.lokimdj.seogi.data.MeetingLibrary
import io.github.lokimdj.seogi.upload.DriveFolder
import io.github.lokimdj.seogi.upload.DriveItem
import io.github.lokimdj.seogi.upload.DriveReader
import io.github.lokimdj.seogi.upload.TokenResult
import io.github.lokimdj.seogi.upload.Uploader
import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.JsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import java.io.File

/** Issue #67, truth 5: changes go both ways, the later one stands, nothing is lost. */
class EditsTest {
    @get:Rule val tmp = TemporaryFolder()

    private fun e(key: String, v: Any, at: Long) =
        Edit(key, if (v is Boolean) JsonPrimitive(v) else JsonPrimitive(v.toString()), at)

    @Test fun theLaterChangeStandsAndTheOtherIsKept() {
        val (winners, others) = Edits.merge(
            mapOf("done:a" to e("done:a", true, 100), "line:0" to e("line:0", "가", 300)),
            mapOf("done:a" to e("done:a", false, 200), "line:0" to e("line:0", "나", 250)),
        )
        assertEquals(JsonPrimitive(false), winners["done:a"]!!.value)
        assertEquals(JsonPrimitive("가"), winners["line:0"]!!.value)
        assertEquals(2, others.size)
        assertTrue(others.first { it.key == "done:a" }.fromComputer)
        assertFalse(others.first { it.key == "line:0" }.fromComputer)
    }

    /** Both sides decide a tie the same way: the computer's stands. */
    @Test fun atTheSameMomentTheComputerStands() {
        val (winners, _) = Edits.merge(mapOf("done:a" to e("done:a", true, 5)), mapOf("done:a" to e("done:a", false, 5)))
        assertEquals(JsonPrimitive(true), winners["done:a"]!!.value)
    }

    @Test fun theSameWordsAreNoConflict() {
        val (_, others) = Edits.merge(mapOf("speaker:1" to e("speaker:1", "가", 1)), mapOf("speaker:1" to e("speaker:1", "가", 2)))
        assertTrue(others.isEmpty())
    }

    @Test fun theMinutesShowWhatStands() {
        val doc = MeetingDoc.parse(
            """{"id":"m","tasks":[{"id":"t1","text":"보내기"}],"sections":[{"kind":"tasks","entries":[{"id":"t1","text":"보내기"}]}],
               "lines":[{"start_ms":0,"speaker":1,"text":"안녕"}],"speakers":[{"id":1,"name":"화자 1"}]}""",
        )!!
        val shown = Edits.overlay(
            doc,
            mapOf("done:t1" to e("done:t1", true, 1), "line:0" to e("line:0", "안녕하세요", 1), "speaker:1" to e("speaker:1", "가", 1)),
        )
        assertTrue(shown.tasks.single().done)
        assertTrue(shown.sections.single().entries.single().done)
        assertEquals("안녕하세요", shown.lines.single().text)
        assertEquals("가", shown.speakerName(1))
    }

    /** The computer's file has the same shape: written by Rust, read here. */
    @Test fun readsTheComputersFile() {
        val f = EditsFile.parse(
            """{"format":1,"device":"computer","meeting":"m","edits":[{"key":"done:t1","value":true,"at_ms":7}],"new":1}""",
        )!!
        assertEquals(7L, f.byKey()["done:t1"]!!.atMs)
        assertTrue(EditsFile.phone("m", f.edits).contains("\"at_ms\":7"))
    }

    private class Reader : DriveReader {
        val text = mutableMapOf<String, String>()
        val files = mutableListOf<DriveItem>()
        var writes = 0
        override fun ensureFolder(token: String) = "ROOT"
        override fun folderUsable(token: String, folderId: String) = true
        override fun folders(token: String, root: String) = listOf(DriveFolder("F", "회의", "m"))
        override fun filesIn(token: String, parents: List<String>) = files.toList()
        override fun readText(token: String, fileId: String) = text.getValue(fileId)
        override fun download(token: String, fileId: String, to: File, onProgress: (Long, Long) -> Unit) = Unit
        override fun writeJson(token: String, folderId: String, name: String, text: String): String {
            writes++
            this.text["P"] = text
            files += DriveItem("P", name, folderId, 1, null, 1, null)
            return "P"
        }
        override fun trash(token: String, fileId: String) = true
        override fun rewriteJson(token: String, fileId: String, text: String) {
            writes++
            this.text[fileId] = text
        }
    }

    @Test fun aTickOnThePhoneGoesToDriveAndTheComputersChangesComeBack() = runBlocking {
        val r = Reader()
        r.text["J"] = """{"id":"m","title":"회의","tasks":[{"id":"t1","text":"보내기"}],"lines":[{"start_ms":0,"text":"안녕"}]}"""
        r.files += DriveItem("J", "회의.json", "F", 1, null, 1, null)
        val lib = MeetingLibrary(File(tmp.root, "m"), r, { TokenResult.Ok("t") }, object : Uploader.FolderCache {
            override var folderId: String? = "ROOT"
        })
        lib.refresh()

        lib.edit("m", "done:t1", JsonPrimitive(true), nowMs = 100)
        assertTrue(lib.view("m")!!.first.tasks.single().done)
        assertEquals(MeetingLibrary.Outcome.DONE, lib.pushEdits())
        assertTrue(r.text["P"]!!.contains("done:t1"))
        assertFalse(lib.phoneEdits("m").pending)
        // Nothing waiting: nothing is sent again.
        lib.pushEdits()
        assertEquals(1, r.writes)

        // The computer took the tick back later: it stands, the phone's stays faint.
        r.text["C"] = """{"format":1,"device":"computer","meeting":"m","edits":[{"key":"done:t1","value":false,"at_ms":200}]}"""
        r.files += DriveItem("C", "고친것-컴퓨터.json", "F", 1, null, 1, null)
        lib.refresh()
        val (doc, others) = lib.view("m")!!
        assertFalse(doc.tasks.single().done)
        assertEquals(JsonPrimitive(true), others.single().value)
        assertFalse(others.single().fromComputer)

        // Taken on the phone, later still: the file is updated, not made again.
        lib.edit("m", "done:t1", JsonPrimitive(true), nowMs = 300)
        lib.pushEdits()
        assertEquals(2, r.writes)
        assertTrue(lib.view("m")!!.first.tasks.single().done)
    }
}
