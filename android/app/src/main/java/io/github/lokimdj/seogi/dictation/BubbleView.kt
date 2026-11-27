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
import android.view.MotionEvent
import android.view.View
import android.view.animation.LinearInterpolator
import io.github.lokimdj.seogi.R

/**
 * The round floating key, drawn by hand (no Compose in an overlay window):
 * warm ink with Seogi's own mark (the launcher icon's single-colour layer),
 * red while listening, a small turning arc while the speech becomes text. Touches go to [onTouch] as they are.
 */
@SuppressLint("ViewConstructor")
class BubbleView(context: Context, private val onTouch: (MotionEvent) -> Unit) : View(context) {
    private val density = resources.displayMetrics.density
    private val fill = Paint(Paint.ANTI_ALIAS_FLAG)
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
    private val mark = context.getDrawable(R.drawable.ic_launcher_monochrome)!!.mutate()
    private val oval = RectF()
    private var turn = 0f
    private val spinner = ValueAnimator.ofFloat(0f, 360f).apply {
        duration = 900
        repeatCount = ValueAnimator.INFINITE
        interpolator = LinearInterpolator()
        addUpdateListener {
            turn = it.animatedValue as Float
            invalidate()
        }
    }

    var mode: BubbleGesture.Mode = BubbleGesture.Mode.IDLE
        set(v) {
            if (field == v) return
            field = v
            if (v == BubbleGesture.Mode.WORKING) spinner.start() else spinner.cancel()
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

    override fun onDraw(canvas: Canvas) {
        val w = width.toFloat()
        val r = w / 2
        val listening = mode == BubbleGesture.Mode.LISTENING
        fill.color = if (listening) RED else INK
        canvas.drawCircle(r, r, r - density, fill)
        if (!listening) canvas.drawCircle(r, r, r - density, ring)
        // The launcher layer is a 108-unit canvas with the mark about 45 units
        // across in the middle; drawn at MARK_SCALE of the key, the mark fills
        // about half of it (the canvas edges fall outside, and are empty).
        val icon = (w * MARK_SCALE).toInt()
        val left = ((w - icon) / 2).toInt()
        mark.setBounds(left, left, left + icon, left + icon)
        mark.colorFilter = PorterDuffColorFilter(if (listening) Color.WHITE else TEXT, PorterDuff.Mode.SRC_IN)
        if (mode == BubbleGesture.Mode.WORKING) {
            mark.alpha = 90
            val inset = 7 * density
            oval.set(inset, inset, w - inset, w - inset)
            canvas.drawArc(oval, turn, 90f, false, arc)
        } else {
            mark.alpha = 255
        }
        mark.draw(canvas)
    }

    @SuppressLint("ClickableViewAccessibility") // the key is also announced and works by tap for TalkBack users
    override fun onTouchEvent(event: MotionEvent): Boolean {
        onTouch(event)
        return true
    }

    override fun onDetachedFromWindow() {
        spinner.cancel()
        super.onDetachedFromWindow()
    }

    private companion object {
        val INK = Color.rgb(0x1E, 0x1B, 0x18)
        val LINE = Color.rgb(0x36, 0x32, 0x2F)
        val TEXT = Color.rgb(0xED, 0xEB, 0xE7)
        val RED = Color.rgb(0xE9, 0x48, 0x3A)
        const val MARK_SCALE = 1.2f
    }
}
