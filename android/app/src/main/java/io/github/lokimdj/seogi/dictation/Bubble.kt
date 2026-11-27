package io.github.lokimdj.seogi.dictation

import kotlin.math.hypot

/**
 * Where the floating key sits, as fractions of the screen, so it stays on the
 * same side and height when the Fold opens or closes or the phone turns.
 * [x] is 0 for the left edge and 1 for the right; [y] runs top to bottom.
 */
data class BubblePlace(val x: Float, val y: Float) {
    /** Top-left pixel of a [size] key on a [w]×[h] screen, kept fully on it. */
    fun toPixels(w: Int, h: Int, size: Int): Pair<Int, Int> {
        val maxX = (w - size).coerceAtLeast(0)
        val maxY = (h - size).coerceAtLeast(0)
        return Pair((x.coerceIn(0f, 1f) * maxX).toInt(), (y.coerceIn(0f, 1f) * maxY).toInt())
    }

    fun encode() = "$x,$y"

    companion object {
        /** Right edge, a little below the middle: out of the way of most fields and keys. */
        val Default = BubblePlace(1f, 0.62f)

        /** After a drag the key goes to the nearer side edge, at the height it was let go. */
        fun afterDrag(px: Int, py: Int, w: Int, h: Int, size: Int): BubblePlace {
            val maxY = (h - size).coerceAtLeast(1)
            val side = if (px + size / 2 < w / 2) 0f else 1f
            return BubblePlace(side, (py.toFloat() / maxY).coerceIn(0f, 1f))
        }

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
