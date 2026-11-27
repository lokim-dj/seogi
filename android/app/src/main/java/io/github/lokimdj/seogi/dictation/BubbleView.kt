package io.github.lokimdj.seogi.dictation

import android.animation.ValueAnimator
import android.annotation.SuppressLint
import android.content.Context
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.PorterDuff
import android.graphics.PorterDuffColorFilter
import android.graphics.RectF
import android.graphics.SweepGradient
import android.view.MotionEvent
import android.view.View
import android.view.animation.DecelerateInterpolator
import android.view.animation.LinearInterpolator
import android.view.animation.OvershootInterpolator
import io.github.lokimdj.seogi.R

/**
 * The round floating key, drawn by hand (no Compose in an overlay window):
 * warm ink with Seogi's own mark (the launcher icon's single-colour layer).
 *
 * How it moves:
 * - pressed, it gives a little under the finger, and springs back;
 * - listening, the mark turns and a red tail runs round the rim (the ink
 *   stays, so the key never turns into a red blob);
 * - while the speech becomes text, the mark fades and a light arc turns;
 * - while the user types by hand ([stepBack]), it shrinks and fades so it is
 *   out of the way, and comes back when the typing stops.
 *
 * The face is drawn at [FACE] of the window, so the spring has room to grow
 * without being cut off. Touches go to [onTouch] as they are.
 */
@SuppressLint("ViewConstructor")
class BubbleView(context: Context, private val onTouch: (MotionEvent) -> Unit) : View(context) {
    private val density = resources.displayMetrics.density
    private val fill = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = INK }
    private val ring = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        style = Paint.Style.STROKE
        strokeWidth = 1 * density
        color = LINE
    }
    private val arc = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        style = Paint.Style.STROKE
        strokeWidth = 2.2f * density
        strokeCap = Paint.Cap.ROUND
        color = TEXT
    }
    private val tail = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        style = Paint.Style.STROKE
        strokeWidth = 2.6f * density
        strokeCap = Paint.Cap.ROUND
    }
    private val mark = context.getDrawable(R.drawable.ic_launcher_monochrome)!!.mutate()
    private val oval = RectF()
    private var turn = 0f
    private val spinner = ValueAnimator.ofFloat(0f, 360f).apply {
        repeatCount = ValueAnimator.INFINITE
        interpolator = LinearInterpolator()
        addUpdateListener {
            turn = it.animatedValue as Float
            invalidate()
        }
    }

    /** True while the user types by hand: the key is small and faint. */
    private var steppedBack = false
    private var pressed = false

    var mode: BubbleGesture.Mode = BubbleGesture.Mode.IDLE
        set(v) {
            if (field == v) return
            field = v
            spinner.cancel()
            when (v) {
                BubbleGesture.Mode.LISTENING -> { spinner.duration = LISTEN_TURN_MS; spinner.start() }
                BubbleGesture.Mode.WORKING -> { spinner.duration = WORK_TURN_MS; spinner.start() }
                BubbleGesture.Mode.IDLE -> turn = 0f
            }
            if (v != BubbleGesture.Mode.IDLE) steppedBack = false
            settle()
            contentDescription = context.getString(
                when (v) {
                    BubbleGesture.Mode.IDLE -> R.string.bubble_idle
                    BubbleGesture.Mode.LISTENING -> R.string.bubble_listening
                    BubbleGesture.Mode.WORKING -> R.string.bubble_working
                },
            )
            invalidate()
        }

    init {
        contentDescription = context.getString(R.string.bubble_idle)
    }

    /** The finger came down: the key gives a little. */
    fun press() {
        pressed = true
        steppedBack = false
        animate().cancel()
        animate().scaleX(PRESSED).scaleY(PRESSED).alpha(1f)
            .setDuration(90).setInterpolator(DecelerateInterpolator()).start()
    }

    /** The finger is gone: the key springs back to its size. */
    fun release() {
        pressed = false
        settle()
    }

    /** The user types by hand ([on]) or stopped: step out of the way, or come back. */
    fun stepBack(on: Boolean) {
        if (pressed || mode != BubbleGesture.Mode.IDLE || steppedBack == on) return
        steppedBack = on
        settle()
    }

    private fun settle() {
        if (pressed) return
        animate().cancel()
        if (steppedBack) {
            animate().scaleX(BACK).scaleY(BACK).alpha(BACK_ALPHA)
                .setDuration(180).setInterpolator(DecelerateInterpolator()).start()
        } else {
            animate().scaleX(1f).scaleY(1f).alpha(1f)
                .setDuration(380).setInterpolator(OvershootInterpolator(3.2f)).start()
        }
    }

    override fun onSizeChanged(w: Int, h: Int, oldw: Int, oldh: Int) {
        super.onSizeChanged(w, h, oldw, oldh)
        val c = w / 2f
        tail.shader = SweepGradient(c, c, intArrayOf(Color.TRANSPARENT, Color.TRANSPARENT, RED), floatArrayOf(0f, 0.35f, 1f))
    }

    override fun onDraw(canvas: Canvas) {
        val w = width.toFloat()
        val c = w / 2
        val r = c * FACE
        canvas.drawCircle(c, c, r, fill)
        canvas.drawCircle(c, c, r, ring)

        val icon = (r * 2 * MARK_SCALE).toInt()
        val left = ((w - icon) / 2).toInt()
        mark.setBounds(left, left, left + icon, left + icon)
        mark.colorFilter = PorterDuffColorFilter(TEXT, PorterDuff.Mode.SRC_IN)
        val inset = 2.4f * density
        oval.set(c - r + inset, c - r + inset, c + r - inset, c + r - inset)
        when (mode) {
            BubbleGesture.Mode.LISTENING -> {
                // A red tail runs round the rim, fading behind its head.
                canvas.save()
                canvas.rotate(turn * 1.6f, c, c)
                canvas.drawArc(oval, 0f, 359f, false, tail)
                canvas.restore()
                // The mark turns slowly, so speaking feels alive.
                mark.alpha = 255
                canvas.save()
                canvas.rotate(turn, c, c)
                mark.draw(canvas)
                canvas.restore()
            }
            BubbleGesture.Mode.WORKING -> {
                mark.alpha = 90
                mark.draw(canvas)
                canvas.drawArc(oval, turn, 90f, false, arc)
            }
            BubbleGesture.Mode.IDLE -> {
                mark.alpha = 255
                mark.draw(canvas)
            }
        }
    }

    @SuppressLint("ClickableViewAccessibility") // the key is also announced and works by tap for TalkBack users
    override fun onTouchEvent(event: MotionEvent): Boolean {
        onTouch(event)
        return true
    }

    override fun onDetachedFromWindow() {
        spinner.cancel()
        animate().cancel()
        super.onDetachedFromWindow()
    }

    private companion object {
        val INK = Color.rgb(0x1E, 0x1B, 0x18)
        val LINE = Color.rgb(0x36, 0x32, 0x2F)
        val TEXT = Color.rgb(0xED, 0xEB, 0xE7)
        val RED = Color.rgb(0xE9, 0x48, 0x3A)

        /** The face's radius as a share of the window's half: room for the spring's overshoot. */
        const val FACE = 0.86f

        /**
         * The launcher layer is a 108-unit canvas with the mark about 45 units
         * across in the middle; drawn at this share of the face, the mark
         * fills about half of it (the canvas edges fall outside, and are empty).
         */
        const val MARK_SCALE = 1.2f
        const val PRESSED = 0.86f
        const val BACK = 0.55f
        const val BACK_ALPHA = 0.45f
        const val LISTEN_TURN_MS = 2400L
        const val WORK_TURN_MS = 900L
    }
}
