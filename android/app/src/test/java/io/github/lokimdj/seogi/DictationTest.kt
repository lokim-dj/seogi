package io.github.lokimdj.seogi

import io.github.lokimdj.seogi.dictation.BubbleGesture
import io.github.lokimdj.seogi.dictation.BubbleGesture.Act
import io.github.lokimdj.seogi.dictation.BubbleGesture.Mode
import io.github.lokimdj.seogi.dictation.BubblePlace
import io.github.lokimdj.seogi.dictation.DictationModel
import io.github.lokimdj.seogi.dictation.InnerField
import io.github.lokimdj.seogi.dictation.InsertTarget
import io.github.lokimdj.seogi.dictation.Inserter
import io.github.lokimdj.seogi.dictation.KeyPresence
import io.github.lokimdj.seogi.dictation.KeyRule
import io.github.lokimdj.seogi.dictation.Typing
import io.github.lokimdj.seogi.dictation.TextField
import io.github.lokimdj.seogi.dictation.Loaded
import io.github.lokimdj.seogi.dictation.OwnWrite
import io.github.lokimdj.seogi.dictation.MeetingCheck
import io.github.lokimdj.seogi.dictation.SetupState
import io.github.lokimdj.seogi.dictation.SetupStep
import io.github.lokimdj.seogi.dictation.Speech
import io.github.lokimdj.seogi.dictation.SpeechEngine
import io.github.lokimdj.seogi.dictation.TextInsert
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.long
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

class DictationTest {
    private val rate = SpeechEngine.RATE

    // --- sound in pieces, text joined ---

    @Test fun shortSpeechIsOnePiece() {
        val pcm = FloatArray(rate * 5) { 0.1f }
        val pieces = Speech.pieces(pcm)
        assertEquals(1, pieces.size)
        assertEquals(pcm.size, pieces[0].size)
    }

    @Test fun longSpeechIsCutAtTheQuietPart() {
        // 40 s of "speech" with a silent half second at 18 s.
        val pcm = FloatArray(rate * 40) { 0.3f }
        for (i in rate * 18 until rate * 18 + rate / 2) pcm[i] = 0f
        val pieces = Speech.pieces(pcm)
        assertEquals(2, pieces.size)
        assertEquals(pcm.size, pieces.sumOf { it.size })
        val cut = pieces[0].size
        assertTrue("cut at ${cut.toDouble() / rate} s", cut in rate * 18..rate * 18 + rate / 2)
        assertTrue(pieces.all { it.size <= Speech.MAX_PIECE_SAMPLES })
    }

    @Test fun twoMinutesStayUnderTheWindow() {
        val pcm = FloatArray(Speech.MAX_SAMPLES) { if (it % 7 == 0) 0.5f else -0.2f }
        val pieces = Speech.pieces(pcm)
        assertEquals(pcm.size, pieces.sumOf { it.size })
        assertTrue(pieces.all { it.size in 1..Speech.MAX_PIECE_SAMPLES })
    }

    @Test fun joinKeepsPunctuationAndDropsEmptyPieces() {
        assertEquals("안녕하세요. 내일 세 시에 봬요!", Speech.join(listOf(" 안녕하세요. ", "", "내일 세 시에 봬요!")))
        assertEquals("", Speech.join(listOf(" ", "")))
    }

    /** What is typed has the numbers written as the computer writes them, and keeps its punctuation. */
    @Test fun writtenTextHasDigitsAndKeepsPunctuation() {
        assertEquals("안녕하세요. 내일 3시에 봬요!", Speech.written(listOf(" 안녕하세요. ", "", "내일 세 시에 봬요!")))
        assertEquals("첫 번째 두 번째 세 번째", Speech.written(listOf("첫 번째 두 번째 세 번째")))
        assertEquals("", Speech.written(listOf(" ", "")))
    }

    // --- putting text into a field ---

    @Test fun insertAtTheCursorWithASpace() {
        val m = TextInsert.merge("회의는 끝", 3, 3, "내일")
        assertEquals("회의는 내일 끝", m.text)
        assertEquals(6, m.cursor)
    }

    @Test fun insertIntoAnEmptyField() {
        assertEquals(TextInsert.merge("", -1, -1, "안녕하세요.").text, "안녕하세요.")
    }

    @Test fun unknownCursorMeansTheEnd() {
        val m = TextInsert.merge("메모", -1, -1, "추가")
        assertEquals("메모 추가", m.text)
        assertEquals(5, m.cursor)
    }

    @Test fun selectionIsReplaced() {
        val m = TextInsert.merge("오늘 회의", 3, 5, "점심")
        assertEquals("오늘 점심", m.text)
        assertEquals(5, m.cursor)
    }

    @Test fun noExtraSpaceAfterASpaceOrBeforePunctuation() {
        assertEquals("오늘 내일", TextInsert.merge("오늘 ", 3, 3, "내일").text)
        assertEquals("오늘.", TextInsert.merge("오늘", 2, 2, ".").text)
        assertEquals("첫 줄\n둘째", TextInsert.merge("첫 줄\n", 5, 5, "둘째").text)
    }

    @Test fun reversedOrOutOfRangeSelectionIsSafe() {
        // Start after end: read as the same selection the right way round (the space is replaced).
        assertEquals("가 나다", TextInsert.merge("가 다", 2, 1, "나").text)
        assertEquals("abc 끝", TextInsert.merge("abc", 10, 20, "끝").text)
    }

    /** A field in another app that can refuse a whole new text, a paste, or both. */
    private class FakeField(
        override var text: String,
        override val selectionStart: Int,
        override val selectionEnd: Int = selectionStart,
        override val editable: Boolean = true,
        override val password: Boolean = false,
        val takesText: Boolean = true,
        val takesPaste: Boolean = true,
    ) : TextField {
        var cursorAt = -1
        var pasted: String? = null
        override fun setText(text: String): Boolean {
            if (takesText) this.text = text
            return takesText
        }
        override fun setCursor(at: Int) { cursorAt = at }
        override fun paste(text: String): Boolean {
            pasted = text
            if (takesPaste) this.text = this.text.substring(0, selectionStart) + text + this.text.substring(selectionEnd)
            return takesPaste
        }
    }

    @Test fun textLandsAtTheCursorOfTheFocusedField() {
        val f = FakeField("내일 보자", 2)
        assertTrue(Inserter.insert(f, "아침에"))
        assertEquals("내일 아침에 보자", f.text)
        assertEquals(6, f.cursorAt)
    }

    @Test fun aFieldThatRefusesNewTextGetsAPaste() {
        val f = FakeField("", 0, takesText = false)
        assertTrue(Inserter.insert(f, "안녕하세요."))
        assertEquals("안녕하세요.", f.pasted)
        assertEquals("안녕하세요.", f.text)
    }

    @Test fun aPasswordFieldIsPastedIntoNeverRewritten() {
        // Accessibility shows a password as dots: rewriting it whole would put the dots in.
        val f = FakeField("•••", 3, password = true)
        assertTrue(Inserter.insert(f, "abc"))
        assertEquals("abc", f.pasted)
        assertEquals("•••abc", f.text) // the fake's own text; the real field keeps its hidden letters
    }

    @Test fun aMeetingIsSeogisOwnFreshStreamWhileSomethingRecords() {
        val dir = kotlin.io.path.createTempDirectory("rec").toFile()
        val now = System.currentTimeMillis()
        assertFalse("nothing recording", MeetingCheck.recording(dir, now, anyCapture = true))
        val stream = File(dir, "20261127-1405.aac").apply { writeBytes(ByteArray(10)); setLastModified(now - 2_000) }
        assertTrue(MeetingCheck.recording(dir, now, anyCapture = true))
        assertFalse("no capture: a stream left by a crash", MeetingCheck.recording(dir, now, anyCapture = false))
        stream.setLastModified(now - 60_000)
        assertFalse("a stale stream and another app's call", MeetingCheck.recording(dir, now, anyCapture = true))
        assertFalse(MeetingCheck.recording(File(dir, "missing"), now, anyCapture = true))
    }

    @Test fun noFieldToTakeItMeansCopy() {
        assertFalse(Inserter.insert(FakeField("글", 1, editable = false), "말"))
        assertFalse(Inserter.insert(FakeField("글", 1, takesText = false, takesPaste = false), "말"))
    }

    // --- the key's place ---

    @Test fun placeStaysOnScreenAndSurvivesTheFold() {
        val p = BubblePlace(1f, 0.5f)
        assertEquals(Pair(1000 - 100, 450), p.toPixels(1000, 1000, 100))
        // Unfolded: wider screen, still on the right edge at the same height.
        assertEquals(Pair(2000 - 100, 450), p.toPixels(2000, 1000, 100))
    }

    @Test fun dragLeavesTheKeyWhereItWasLetGo() {
        val p = BubblePlace.afterDrag(450, 900, 1000, 1000, 100, margin = 50)
        assertEquals(0.5f, p.x)
        assertEquals(1f, p.y)
        assertEquals(Pair(450, 900), p.toPixels(1000, 1000, 100, margin = 50))
    }

    @Test fun seogisOwnWritingIsNotTyping() {
        assertFalse(OwnWrite.isTyping(now = 10_500, wroteAt = 10_000))
        assertFalse(OwnWrite.isTyping(now = 11_500, wroteAt = 10_000))
        assertTrue(OwnWrite.isTyping(now = 11_501, wroteAt = 10_000))
        // Nothing written yet: every change is the user's.
        assertTrue(OwnWrite.isTyping(now = 5_000, wroteAt = 0))
    }

    @Test fun keyStaysOffTheBackGestureStrips() {
        // Pushed to either side, the key still leaves the margin free.
        assertEquals(50, BubblePlace(0f, 0f).toPixels(1000, 1000, 100, margin = 50).first)
        assertEquals(1000 - 100 - 50, BubblePlace(1f, 0f).toPixels(1000, 1000, 100, margin = 50).first)
        // A screen too narrow for the margin still shows the whole key.
        assertEquals(0, BubblePlace(0f, 0f).toPixels(100, 1000, 100, margin = 50).first)
    }

    @Test fun placeReadsBackAndBadTextGivesTheDefault() {
        assertEquals(BubblePlace(0f, 0.25f), BubblePlace.decode(BubblePlace(0f, 0.25f).encode()))
        assertEquals(BubblePlace.Default, BubblePlace.decode(null))
        assertEquals(BubblePlace.Default, BubblePlace.decode("x,y"))
        assertEquals(BubblePlace(1f, 0f), BubblePlace.decode("3,-2"))
    }

    // --- what a touch means ---

    @Test fun holdToTalk() {
        val g = BubbleGesture(holdMs = 400, slop = 10f)
        assertEquals(listOf(Act.Start), g.down(0, 50f, 50f, Mode.IDLE))
        assertEquals(emptyList<Act>(), g.move(53f, 52f)) // a finger is never still
        assertEquals(listOf(Act.Stop), g.up(1500))
    }

    @Test fun tapToStartTapToStop() {
        val g = BubbleGesture(holdMs = 400, slop = 10f)
        assertEquals(listOf(Act.Start), g.down(0, 50f, 50f, Mode.IDLE))
        assertEquals(emptyList<Act>(), g.up(120))
        assertEquals(emptyList<Act>(), g.down(3000, 50f, 50f, Mode.LISTENING))
        assertEquals(emptyList<Act>(), g.move(80f, 80f)) // the stopping touch does not drag
        assertEquals(listOf(Act.Stop), g.up(3100))
    }

    @Test fun dragThrowsAwayTheListeningItBegan() {
        val g = BubbleGesture(holdMs = 400, slop = 10f)
        g.down(0, 50f, 50f, Mode.IDLE)
        assertEquals(listOf(Act.Cancel, Act.Move(0f, 40f)), g.move(50f, 90f))
        assertEquals(listOf(Act.Move(0f, 60f)), g.move(50f, 110f))
        assertEquals(listOf(Act.DragEnd), g.up(2000))
    }

    @Test fun whileWritingATouchOnlyDrags() {
        val g = BubbleGesture(holdMs = 400, slop = 10f)
        assertEquals(emptyList<Act>(), g.down(0, 50f, 50f, Mode.WORKING))
        assertEquals(emptyList<Act>(), g.up(1000))
        g.down(0, 50f, 50f, Mode.WORKING)
        assertEquals(listOf(Act.Move(30f, 0f)), g.move(80f, 50f))
        assertEquals(listOf(Act.DragEnd), g.up(500))
    }

    @Test fun aCancelledTouchDropsItsListening() {
        val g = BubbleGesture()
        g.down(0, 0f, 0f, Mode.IDLE)
        assertEquals(listOf(Act.Cancel), g.cancel())
    }

    // --- which field the text goes into ---

    private data class F(val name: String, val editable: Boolean = true, val onScreen: Boolean = true)

    private fun pick(focused: F?, remembered: F?) =
        InsertTarget.pick(focused, remembered, editable = { it.editable }, onScreen = { it.onScreen })

    @Test fun theFocusedFieldComesFirst() {
        val c = pick(F("now"), F("before"))
        assertEquals(F("now"), c.field)
        assertFalse(c.refocus)
    }

    @Test fun whenTheFocusMovedTheRememberedFieldIsUsedAndFocused() {
        val none = pick(null, F("before"))
        assertEquals(F("before"), none.field)
        assertTrue(none.refocus)
        val button = pick(F("button", editable = false), F("before"))
        assertEquals(F("before"), button.field)
        assertTrue(button.refocus)
    }

    @Test fun withNoFieldToTakeItTheTextIsCopiedAndTheLogSaysWhy() {
        assertEquals(null, pick(null, null).field)
        assertEquals("no focused field, none remembered", pick(null, null).why)
        val gone = pick(null, F("before", onScreen = false))
        assertEquals(null, gone.field)
        assertTrue(gone.why.contains("gone"))
        assertEquals(null, pick(null, F("before", editable = false)).field)
        val same = F("label", editable = false)
        assertEquals(null, pick(same, same).field)
    }

    @Test fun aFocusedFrameThatTakesNoTextGivesTheFieldInsideIt() {
        val frame = F("frame", editable = false)
        val body = F("body")
        val c = InsertTarget.pick(frame, null, editable = { it.editable }, onScreen = { it.onScreen }, inside = { body })
        assertEquals(body, c.field)
        assertFalse(c.refocus)
        // The field the user was typing in still comes before it.
        val r = InsertTarget.pick(frame, F("before"), editable = { it.editable }, onScreen = { it.onScreen }, inside = { body })
        assertEquals(F("before"), r.field)
        val none = InsertTarget.pick(frame, null, editable = { it.editable }, onScreen = { it.onScreen }, inside = { null })
        assertEquals(null, none.field)
        assertEquals("focused field takes no text, none remembered, none inside it", none.why)
    }

    // --- the field inside a focused frame ---

    private data class N(
        val name: String,
        val editable: Boolean = false,
        val focused: Boolean = false,
        val cursor: Boolean = false,
        val kids: List<N> = emptyList(),
    )

    private fun inner(top: N, limit: Int = 300) = InnerField.pick(
        top, children = { it.kids }, editable = { it.editable }, focused = { it.focused }, hasCursor = { it.cursor }, limit = limit,
    )?.name

    @Test fun theOnlyFieldInsideTheFrameIsTaken() {
        // Samsung Notes' body: the focus on a ScrollView, the EditText within, no cursor reported.
        val note = N("scroll", kids = listOf(N("layout", kids = listOf(N("body", editable = true)))))
        assertEquals("body", inner(note))
    }

    @Test fun amongSeveralFieldsTheOneWithTheCursorIsTaken() {
        val focusedOne = N("page", kids = listOf(N("title", editable = true), N("body", editable = true, focused = true)))
        assertEquals("body", inner(focusedOne))
        val cursorOne = N("page", kids = listOf(N("title", editable = true), N("body", editable = true, cursor = true)))
        assertEquals("body", inner(cursorOne))
        // The input focus outranks a cursor left behind in another field.
        val both = N("page", kids = listOf(N("title", editable = true, cursor = true), N("body", editable = true, focused = true, cursor = true)))
        assertEquals("body", inner(both))
    }

    @Test fun severalFieldsWithNothingToTellThemApartAreLeftAlone() {
        assertEquals(null, inner(N("page", kids = listOf(N("a", editable = true), N("b", editable = true)))))
        assertEquals(null, inner(N("page", kids = listOf(N("a", editable = true, cursor = true), N("b", editable = true, cursor = true)))))
        assertEquals(null, inner(N("page", kids = listOf(N("label"), N("button")))))
        assertEquals(null, inner(N("empty")))
    }

    @Test fun aFieldsOwnPartsAreNotCountedAsFields() {
        val field = N("field", editable = true, kids = listOf(N("inner", editable = true)))
        assertEquals("field", inner(N("frame", kids = listOf(field))))
    }

    @Test fun aWalkCutShortTrustsOnlyTheFocus() {
        val many = (1..10).map { N("row$it") }
        val unseen = N("late", editable = true)
        // One field, then a long list: seen whole it is the only one; cut short, it may not be.
        val lone = N("page", kids = listOf(N("early", editable = true)) + many)
        assertEquals("early", inner(lone))
        assertEquals(null, inner(lone, limit = 5))
        val cursorEarly = N("page", kids = listOf(N("early", editable = true, cursor = true)) + many + unseen)
        assertEquals("early", inner(cursorEarly))
        // Not every part was seen: an unseen field could have a cursor too.
        assertEquals(null, inner(cursorEarly, limit = 5))
        val focusedEarly = N("page", kids = listOf(N("early", editable = true, focused = true)) + many + unseen)
        assertEquals("early", inner(focusedEarly, limit = 5))
    }

    // --- when the key is on the screen ---

    @Test fun typingMeansTheKeyboardIsUp() {
        assertTrue(Typing.from(keyboardShowing = true, editableFocused = false, hardKeyboard = false))
        // The keyboard closed, even with the cursor still in the field: the user stopped typing.
        assertFalse(Typing.from(keyboardShowing = false, editableFocused = true, hardKeyboard = false))
        assertFalse(Typing.from(keyboardShowing = false, editableFocused = false, hardKeyboard = false))
        // No window list, or a keyboard plugged in: the focused field decides.
        assertTrue(Typing.from(keyboardShowing = null, editableFocused = true, hardKeyboard = false))
        assertFalse(Typing.from(keyboardShowing = null, editableFocused = false, hardKeyboard = false))
        assertTrue(Typing.from(keyboardShowing = false, editableFocused = true, hardKeyboard = true))
    }

    @Test fun theKeyShowsOnlyWhileTyping() {
        assertTrue(KeyRule.wanted(Mode.IDLE, userHidden = false, meeting = false, followsTyping = true, typing = true))
        assertFalse(KeyRule.wanted(Mode.IDLE, userHidden = false, meeting = false, followsTyping = true, typing = false))
    }

    @Test fun listeningOrWritingKeepsTheKeyWhateverTheFocusDoes() {
        for (m in listOf(Mode.LISTENING, Mode.WORKING)) {
            assertTrue(KeyRule.wanted(m, userHidden = false, meeting = false, followsTyping = true, typing = false))
            assertTrue(KeyRule.wanted(m, userHidden = false, meeting = true, followsTyping = true, typing = false))
        }
    }

    @Test fun aMeetingOrTheNotificationHidesTheKey() {
        assertFalse(KeyRule.wanted(Mode.IDLE, userHidden = false, meeting = true, followsTyping = true, typing = true))
        assertFalse(KeyRule.wanted(Mode.IDLE, userHidden = false, meeting = true, followsTyping = false, typing = false))
        assertFalse(KeyRule.wanted(Mode.IDLE, userHidden = true, meeting = false, followsTyping = true, typing = true))
        assertFalse(KeyRule.wanted(Mode.LISTENING, userHidden = true, meeting = false, followsTyping = true, typing = true))
    }

    @Test fun withoutAccessibilityTheKeyStaysUp() {
        assertTrue(KeyRule.wanted(Mode.IDLE, userHidden = false, meeting = false, followsTyping = false, typing = false))
    }

    @Test fun theKeyComesAfterAMomentAndGoesAfterALongerOne() {
        val p = KeyPresence(showAfterMs = 100, hideAfterMs = 800)
        assertEquals(100L, p.update(true, 0))
        assertFalse(p.shown)
        assertEquals(40L, p.update(true, 60))
        assertEquals(0L, p.update(true, 100))
        assertTrue(p.shown)
        assertEquals(null, p.update(true, 150))
        assertEquals(800L, p.update(false, 1000))
        assertEquals(0L, p.update(false, 1800))
        assertFalse(p.shown)
    }

    @Test fun movingFromFieldToFieldDoesNotFlicker() {
        val p = KeyPresence(showAfterMs = 100, hideAfterMs = 800)
        p.update(true, 0)
        p.update(true, 100)
        assertTrue(p.shown)
        // Focus passes through a button on its way to the next field.
        assertEquals(800L, p.update(false, 1000))
        assertEquals(null, p.update(true, 1200))
        assertEquals(null, p.update(true, 2500))
        assertTrue(p.shown)
        // A field that takes the cursor only for a blink never brings the key up.
        val q = KeyPresence(showAfterMs = 100, hideAfterMs = 800)
        assertEquals(100L, q.update(true, 0))
        assertEquals(null, q.update(false, 30))
        assertEquals(null, q.update(false, 500))
        assertFalse(q.shown)
    }

    @Test fun aChoiceFromTheUserIsAtOnce() {
        val p = KeyPresence(showAfterMs = 100, hideAfterMs = 800)
        assertEquals(0L, p.update(true, 0, atOnce = true))
        assertTrue(p.shown)
        assertEquals(0L, p.update(false, 10, atOnce = true))
        assertFalse(p.shown)
    }

    // --- the steps ---

    @Test fun stepsComeInOrderAndSkipWhatIsDone() {
        val none = SetupState(mic = false, notifications = true, overlay = false, accessibility = false, copyOnly = false, modelReady = false)
        assertEquals(SetupStep.MIC, none.current())
        assertEquals(listOf(SetupStep.MIC, SetupStep.OVERLAY, SetupStep.ACCESSIBILITY, SetupStep.MODEL), none.missing())
        val noA11y = none.copy(mic = true, overlay = true)
        assertEquals(SetupStep.ACCESSIBILITY, noA11y.current())
        assertEquals(Pair(3, 4), noA11y.position(SetupStep.ACCESSIBILITY))
        assertEquals(SetupStep.MODEL, noA11y.copy(copyOnly = true).current())
        assertEquals(SetupStep.READY, noA11y.copy(accessibility = true, modelReady = true).current())
    }

    @Test fun cpuNeedsDotProductAndInt8Matrix() {
        val good = "processor\t: 0\nFeatures\t: fp asimd evtstrm aes asimddp i8mm bf16\nprocessor\t: 1\nFeatures\t: fp asimd asimddp i8mm\n"
        assertTrue(SpeechEngine.cpuSupported(good))
        assertFalse(SpeechEngine.cpuSupported("Features\t: fp asimd asimddp\n"))
        assertFalse(SpeechEngine.cpuSupported(good + "Features\t: fp asimd\n"))
        assertFalse(SpeechEngine.cpuSupported(""))
    }

    // --- the model ---

    /** The phone downloads exactly what the desktop catalog lists, so both read the same model. */
    @Test fun modelMatchesTheDesktopCatalog() {
        val catalog = File("../../src-tauri/src/catalog/catalog.json")
        assertTrue("catalog at ${catalog.absolutePath}", catalog.exists())
        val root = Json.parseToJsonElement(catalog.readText()).jsonObject
        val models = root["models"]?.jsonArray ?: root.values.first { it is kotlinx.serialization.json.JsonArray }.jsonArray
        val qwen = models.map { it.jsonObject }.first { it["id"]?.jsonPrimitive?.content == "Qwen3-ASR-0.6B" }
        assertEquals(DictationModel.REPOSITORY, qwen["repository"]!!.jsonPrimitive.content)
        assertEquals(DictationModel.REVISION, qwen["revision"]!!.jsonPrimitive.content)
        val q8 = qwen["files"]!!.jsonObject["Q8_0"]!!.jsonObject
        assertEquals(DictationModel.FILE_NAME, q8["filename"]!!.jsonPrimitive.content)
        assertEquals(DictationModel.SIZE, q8["size"]!!.jsonPrimitive.long)
        assertEquals(DictationModel.SHA256, q8["sha256"]!!.jsonPrimitive.content)
        assertEquals(
            "https://huggingface.co/${DictationModel.REPOSITORY}/resolve/${DictationModel.REVISION}/${DictationModel.FILE_NAME}",
            DictationModel.URL,
        )
    }

    // --- loaded once, let go when idle ---

    private class Thing(val n: Int) : AutoCloseable {
        var closed = false
        override fun close() { closed = true }
    }

    @OptIn(ExperimentalCoroutinesApi::class)
    @Test fun modelLoadsOnceAndIsLetGoWhenIdle() = runTest {
        val opened = mutableListOf<Thing>()
        val loaded = Loaded(backgroundScope, idleMs = 180_000) { Thing(opened.size).also { opened += it } }
        loaded.warm()
        assertEquals(1, loaded.use { it.n + 1 })
        advanceTimeBy(170_000)
        loaded.use { }
        assertEquals("used again before the idle time: not reopened", 1, opened.size)
        advanceTimeBy(179_000)
        runCurrent()
        assertTrue(loaded.isOpen)
        advanceTimeBy(2_000)
        runCurrent()
        assertFalse(loaded.isOpen)
        assertTrue(opened[0].closed)
        loaded.use { }
        assertEquals(2, opened.size)
        loaded.close()
        assertTrue(opened[1].closed)
    }

    @Test fun aFailedOpenLeavesNothingHalfOpen() = runTest {
        var tries = 0
        val loaded = Loaded(backgroundScope, idleMs = 1000) { tries++; if (tries == 1) error("no memory") else Thing(tries) }
        assertTrue(runCatching { loaded.warm() }.isFailure)
        assertFalse(loaded.isOpen)
        loaded.warm()
        assertTrue(loaded.isOpen)
    }
}
