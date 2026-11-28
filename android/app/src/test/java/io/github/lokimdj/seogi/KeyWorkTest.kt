package io.github.lokimdj.seogi

import io.github.lokimdj.seogi.dictation.BubbleGesture
import io.github.lokimdj.seogi.dictation.BubbleGesture.Act
import io.github.lokimdj.seogi.dictation.BubbleGesture.Mode
import io.github.lokimdj.seogi.dictation.KeyPresence
import io.github.lokimdj.seogi.dictation.KeyRule
import io.github.lokimdj.seogi.dictation.KeyWork
import io.github.lokimdj.seogi.dictation.KeyWork.End
import io.github.lokimdj.seogi.dictation.Speech
import io.github.lokimdj.seogi.dictation.SpeechEngine
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** #96: the key always comes back to rest, and never stays on a screen with no field. */
class KeyWorkTest {
    // --- the time caps ---

    @Test fun makingTextHasACapThatGrowsWithTheSpeech() {
        assertEquals(KeyWork.WORK_BASE_MS, KeyWork.workCapMs(0))
        assertEquals(KeyWork.WORK_BASE_MS + 2_000, KeyWork.workCapMs(2 * SpeechEngine.RATE))
        assertEquals(KeyWork.WORK_BASE_MS, KeyWork.workCapMs(-5)) // never below the base
        // The longest speech still gets a cap, and a generous one.
        val longest = KeyWork.workCapMs(Speech.MAX_SAMPLES)
        assertEquals(KeyWork.WORK_BASE_MS + 120_000, longest)
    }

    @Test fun listeningEndsSoonAfterTheMicrophoneWouldHaveFilledUp() {
        val full = Speech.MAX_SAMPLES * 1000L / SpeechEngine.RATE
        assertTrue(KeyWork.LISTEN_CAP_MS > full)
        assertTrue(KeyWork.LISTEN_CAP_MS <= full + 10_000)
    }

    @Test fun aRunBehindAnUnfinishedOneDoesNotTurnTheKey() {
        assertTrue(KeyWork.turns(0))
        assertFalse(KeyWork.turns(1)) // the engine is still on a run the key stopped waiting for
        assertFalse(KeyWork.turns(3))
    }

    // --- what becomes of the text ---

    @Test fun whileTheKeyWaitsTheTextIsTypedOrTheTroubleSaid() {
        assertEquals(End.TYPE, KeyWork.end("세 시에 봬요", waited = true))
        assertEquals(End.HEARD_NOTHING, KeyWork.end("", waited = true)) // an empty result rests the key too
        assertEquals(End.FAILED, KeyWork.end(null, waited = true)) // an engine error or exception
    }

    @Test fun textThatComesAfterTheKeyStoppedWaitingIsCopiedNeverTyped() {
        assertEquals(End.COPY, KeyWork.end("세 시에 봬요", waited = false))
        assertEquals(End.FAILED, KeyWork.end(null, waited = false)) // the user was promised a copy: say it failed
        assertEquals(End.QUIET, KeyWork.end("", waited = false))
    }

    // --- a tap puts a turning key to rest ---

    @Test fun aTapOnTheTurningKeyStopsWaiting() {
        val g = BubbleGesture(holdMs = 400, slop = 10f)
        assertEquals(emptyList<Act>(), g.down(0, 50f, 50f, Mode.WORKING))
        assertEquals(listOf(Act.LetGo), g.up(80))
        // A long press too.
        g.down(1000, 50f, 50f, Mode.WORKING)
        assertEquals(listOf(Act.LetGo), g.up(3000))
    }

    @Test fun draggingTheTurningKeyOnlyMovesIt() {
        val g = BubbleGesture(holdMs = 400, slop = 10f)
        g.down(0, 50f, 50f, Mode.WORKING)
        assertEquals(listOf(Act.Move(0f, 40f)), g.move(50f, 90f))
        assertEquals(listOf(Act.DragEnd), g.up(500))
        // A touch the system takes away does nothing to the work.
        g.down(0, 50f, 50f, Mode.WORKING)
        assertEquals(emptyList<Act>(), g.cancel())
    }

    @Test fun theNextTouchAfterRestingStartsAfresh() {
        val g = BubbleGesture(holdMs = 400, slop = 10f)
        g.down(0, 50f, 50f, Mode.WORKING)
        g.up(80)
        assertEquals(listOf(Act.Start), g.down(500, 50f, 50f, Mode.IDLE))
        assertEquals(emptyList<Act>(), g.up(600))
    }

    // --- no field, no key, whatever it is doing ---

    @Test fun withNoFieldTheKeyHidesInEveryState() {
        for (m in Mode.values()) {
            for (meeting in listOf(false, true)) {
                assertFalse("$m meeting=$meeting", KeyRule.wanted(m, userHidden = false, meeting = meeting, followsTyping = true, typing = false))
            }
        }
    }

    @Test fun aTurningKeyGoesFromTheHomeScreenAfterTheUsualMoment() {
        // The spinning key on the home screen (#96): with the rule, the presence takes it away.
        val p = KeyPresence(showAfterMs = 120, hideAfterMs = 300)
        p.update(KeyRule.wanted(Mode.WORKING, false, false, followsTyping = true, typing = true), 0, atOnce = true)
        assertTrue(p.shown)
        val wait = p.update(KeyRule.wanted(Mode.WORKING, false, false, followsTyping = true, typing = false), 1_000)
        assertEquals(300L, wait)
        assertEquals(0L, p.update(KeyRule.wanted(Mode.WORKING, false, false, followsTyping = true, typing = false), 1_300))
        assertFalse(p.shown)
    }

    @Test fun aListeningKeyWaitsLongerBeforeItGoes() {
        // The keyboard folds away for a moment while the user speaks: the sentence is not cut.
        val p = KeyPresence(showAfterMs = 120, hideAfterMs = 300)
        p.update(true, 0, atOnce = true)
        assertEquals(KeyWork.LISTEN_HIDE_MS, p.update(false, 1_000, hideAfter = KeyWork.LISTEN_HIDE_MS))
        assertEquals(null, p.update(true, 1_800)) // back before the wait ran out: it stayed
        assertTrue(p.shown)
        p.update(false, 2_000, hideAfter = KeyWork.LISTEN_HIDE_MS)
        assertEquals(0L, p.update(false, 2_000 + KeyWork.LISTEN_HIDE_MS, hideAfter = KeyWork.LISTEN_HIDE_MS))
        assertFalse(p.shown)
    }
}
