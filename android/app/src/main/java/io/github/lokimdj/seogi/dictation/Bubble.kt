package io.github.lokimdj.seogi.dictation

import kotlin.math.hypot

/**
 * Where the floating key sits, as fractions of the room it can move in, so it
 * keeps its place when the Fold opens or closes or the phone turns.
 * [x] is 0 for the far left and 1 for the far right; [y] runs top to bottom.
 * The room leaves [margin] pixels free at the left and right, where Android's
 * back gesture starts: a key there would be hard to touch, and a touch could
 * go back instead.
 */
data class BubblePlace(val x: Float, val y: Float) {
    /** Top-left pixel of a [size] key on a [w]×[h] screen, kept fully on it and off the side margins. */
    fun toPixels(w: Int, h: Int, size: Int, margin: Int = 0): Pair<Int, Int> {
        val m = sideMargin(w, size, margin)
        val spanX = (w - size - 2 * m).coerceAtLeast(0)
        val maxY = (h - size).coerceAtLeast(0)
        return Pair(m + (x.coerceIn(0f, 1f) * spanX).toInt(), (y.coerceIn(0f, 1f) * maxY).toInt())
    }

    fun encode() = "$x,$y"

    companion object {
        /** Near the right side, a little below the middle: out of the way of most fields and keys. */
        val Default = BubblePlace(1f, 0.62f)

        /** After a drag the key stays where it was let go. */
        fun afterDrag(px: Int, py: Int, w: Int, h: Int, size: Int, margin: Int = 0): BubblePlace {
            val m = sideMargin(w, size, margin)
            val spanX = (w - size - 2 * m).coerceAtLeast(1)
            val maxY = (h - size).coerceAtLeast(1)
            return BubblePlace(((px - m).toFloat() / spanX).coerceIn(0f, 1f), (py.toFloat() / maxY).coerceIn(0f, 1f))
        }

        /** The side margin, made smaller on a screen too narrow to keep it. */
        private fun sideMargin(w: Int, size: Int, margin: Int): Int =
            margin.coerceIn(0, ((w - size) / 2).coerceAtLeast(0))

        fun decode(s: String?): BubblePlace {
            val parts = s?.trim()?.split(',') ?: return Default
            val x = parts.getOrNull(0)?.toFloatOrNull()
            val y = parts.getOrNull(1)?.toFloatOrNull()
            if (parts.size != 2 || x == null || y == null || x.isNaN() || y.isNaN()) return Default
            return BubblePlace(x.coerceIn(0f, 1f), y.coerceIn(0f, 1f))
        }
    }
}

/**
 * What a touch on the floating key means. Speaking starts the moment the key
 * is touched (no words lost while deciding); then
 * - let go after holding at least [holdMs]: stop and write (hold to talk);
 * - let go sooner: keep listening, and the next touch stops (tap, tap);
 * - move further than [slop] before letting go: it was a drag, the listening
 *   that just began is thrown away and the key follows the finger.
 * While the text is being made a touch only drags.
 */
class BubbleGesture(private val holdMs: Long = 400, private val slop: Float = 24f) {
    sealed interface Act {
        data object Start : Act
        data object Stop : Act
        data object Cancel : Act
        data class Move(val dx: Float, val dy: Float) : Act
        data object DragEnd : Act
    }

    enum class Mode { IDLE, LISTENING, WORKING }

    private var downAt = 0L
    private var downX = 0f
    private var downY = 0f
    private var startedHere = false
    private var stopOnUp = false
    private var dragging = false

    fun down(t: Long, x: Float, y: Float, mode: Mode): List<Act> {
        downAt = t; downX = x; downY = y
        dragging = false
        startedHere = mode == Mode.IDLE
        stopOnUp = mode == Mode.LISTENING
        return if (startedHere) listOf(Act.Start) else emptyList()
    }

    fun move(x: Float, y: Float): List<Act> {
        val dx = x - downX
        val dy = y - downY
        if (!dragging) {
            // A key that is listening from an earlier tap does not move: the touch is meant to stop it.
            if (stopOnUp || hypot(dx, dy) <= slop) return emptyList()
            dragging = true
            return if (startedHere) listOf(Act.Cancel, Act.Move(dx, dy)) else listOf(Act.Move(dx, dy))
        }
        return listOf(Act.Move(dx, dy))
    }

    fun up(t: Long): List<Act> = when {
        dragging -> listOf(Act.DragEnd)
        startedHere && t - downAt >= holdMs -> listOf(Act.Stop)
        startedHere -> emptyList()
        stopOnUp -> listOf(Act.Stop)
        else -> emptyList()
    }

    /** The touch was taken away (the system cancelled it): a listening it began is thrown away. */
    fun cancel(): List<Act> = when {
        dragging -> listOf(Act.DragEnd)
        startedHere -> listOf(Act.Cancel)
        else -> emptyList()
    }
}

/**
 * Whether the user is typing somewhere, from what the accessibility service
 * can see: the on-screen keyboard is up, or (with a keyboard plugged in, or
 * when Android does not list its windows) a text field has the cursor.
 */
object Typing {
    /**
     * [keyboardShowing] is null when the window list could not be read.
     * [editableFocused]: the field with the input focus takes text.
     * [hardKeyboard]: a physical keyboard is in use, so no on-screen one comes up.
     */
    fun from(keyboardShowing: Boolean?, editableFocused: Boolean, hardKeyboard: Boolean): Boolean = when {
        keyboardShowing == true -> true
        keyboardShowing == null || hardKeyboard -> editableFocused
        else -> false
    }
}

/**
 * Whether the floating key should be on the screen right now.
 * - Hidden from the notification: hidden.
 * - Listening or making the text: shown, whatever the focus does, so the
 *   key the user is speaking into never vanishes.
 * - Seogi is recording a meeting: hidden (dictation cannot listen then).
 * - Without the accessibility service there is no way to tell when the user
 *   types ([followsTyping] false): always shown, as before.
 * - Otherwise: only while the user is typing.
 */
object KeyRule {
    fun wanted(
        mode: BubbleGesture.Mode,
        userHidden: Boolean,
        meeting: Boolean,
        followsTyping: Boolean,
        typing: Boolean,
    ): Boolean = when {
        userHidden -> false
        mode != BubbleGesture.Mode.IDLE -> true
        meeting -> false
        !followsTyping -> true
        else -> typing
    }
}

/**
 * Whether a change in a text field is the user typing by hand: changes
 * within [withinMs] of Seogi writing ([wroteAt]) are Seogi's own.
 */
object OwnWrite {
    fun isTyping(now: Long, wroteAt: Long, withinMs: Long = 1_500): Boolean = now - wroteAt > withinMs
}

/**
 * Keeps the key from flickering: it comes up once the wish to show it has held
 * for [showAfterMs] and goes once the wish to hide it has held for
 * [hideAfterMs], so moving the cursor from one field to the next, or the
 * keyboard closing and opening again, leaves it where it is.
 */
class KeyPresence(private val showAfterMs: Long = 120, private val hideAfterMs: Long = 300) {
    /** Whether the key is (to be) on screen. */
    var shown = false
        private set
    private var want = false
    private var since = 0L

    /**
     * Feed what is wanted at [now]. Returns 0 when [shown] just changed (apply
     * it), the milliseconds to wait before calling again, or null when
     * nothing is pending. [atOnce] skips the wait (a choice from the user).
     */
    fun update(wanted: Boolean, now: Long, atOnce: Boolean = false): Long? {
        if (wanted != want) {
            want = wanted
            since = now
        }
        if (want == shown) return null
        val wait = (if (want) showAfterMs else hideAfterMs) - (now - since)
        if (atOnce || wait <= 0) {
            shown = want
            return 0
        }
        return wait
    }
}
