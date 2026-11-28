package io.github.lokimdj.seogi.ui

import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.snap
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.ColorFilter
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.nestedscroll.NestedScrollConnection
import androidx.compose.ui.input.nestedscroll.NestedScrollSource
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.Velocity
import androidx.compose.ui.unit.dp
import io.github.lokimdj.seogi.R

/** The arithmetic of pulling a list down to read it again (issue #102). */
object Pull {
    /** The ring turns this far, in degrees, by the time a pull is long enough. */
    const val FULL_TURN = 270f

    /** The pull after the finger moved [delta]: harder the further it goes, never past [max]. */
    fun drag(pulled: Float, delta: Float, max: Float): Float {
        val next = if (delta > 0f) pulled + delta * (1f - pulled / max) else pulled + delta
        return next.coerceIn(0f, max)
    }

    /** Letting go here reads the list again. */
    fun reads(pulled: Float, threshold: Float): Boolean = pulled >= threshold

    /** How far the ring has turned, following the pull. */
    fun turn(pulled: Float, threshold: Float): Float = (pulled / threshold).coerceIn(0f, 1f) * FULL_TURN
}

/** One turn of the ring while the list is read: slow, so it reads as calm. */
private const val TURN_MS = 1500

/**
 * A list that reads the shared folder again when pulled down. The Seogi
 * mark is the sign: it follows the pull, turns slowly while [reading], then
 * stops and goes. With motion reduced it fades in and out instead of turning.
 * [onPull] decides whether a read really starts (and never two at once);
 * [reading] is true only for a read that came from a pull, so reads on
 * opening the app neither show the ring nor move the list.
 */
@Composable
fun PullToRead(reading: Boolean, onPull: () -> Unit, modifier: Modifier = Modifier, content: @Composable () -> Unit) {
    val density = LocalDensity.current
    val threshold = with(density) { 64.dp.toPx() }
    val max = threshold * 1.5f
    val rest = with(density) { 48.dp.toPx() }
    val reduce = LocalReduceMotion.current
    val pullNow by rememberUpdatedState(onPull)
    val readingNow by rememberUpdatedState(reading)
    val spin = remember { Animatable(0f) }
    var pulled by remember { mutableFloatStateOf(0f) }

    val connection = remember(threshold) {
        object : NestedScrollConnection {
            override fun onPreScroll(available: Offset, source: NestedScrollSource): Offset {
                // Pushed back up: the pull gives way before the list scrolls.
                if (source != NestedScrollSource.UserInput || available.y >= 0f || pulled <= 0f) return Offset.Zero
                val before = pulled
                pulled = Pull.drag(before, available.y, max)
                return Offset(0f, pulled - before)
            }

            override fun onPostScroll(consumed: Offset, available: Offset, source: NestedScrollSource): Offset {
                // At the top and still going down: that is the pull. Not while a pulled read runs.
                if (source != NestedScrollSource.UserInput || available.y <= 0f || readingNow) return Offset.Zero
                pulled = Pull.drag(pulled, available.y, max)
                return Offset(0f, available.y)
            }

            override suspend fun onPreFling(available: Velocity): Velocity {
                if (pulled <= 0f) return Velocity.Zero
                val at = pulled
                pulled = 0f
                spin.snapTo(Pull.turn(at, threshold))
                if (Pull.reads(at, threshold)) pullNow()
                return available
            }
        }
    }

    // While reading: one slow turn after another from where the pull left it.
    // When the read ends this is cancelled, so the ring stops where it is.
    LaunchedEffect(reading, reduce) {
        if (reading && !reduce) {
            while (true) spin.animateTo(spin.value + 360f, tween(TURN_MS, easing = LinearEasing))
        }
    }
    val gap by animateFloatAsState(
        if (reading) rest else pulled,
        if (pulled > 0f && !reading) snap() else tween(250),
        label = "pull-gap",
    )
    val pulse = if (reading && reduce) {
        val t = rememberInfiniteTransition(label = "pull-fade")
        val a by t.animateFloat(0.3f, 1f, infiniteRepeatable(tween(900), RepeatMode.Reverse), label = "pull-alpha")
        a
    } else {
        1f
    }
    val label = if (reading) stringResource(R.string.list_reading) else null

    Box(modifier.clipToBounds().nestedScroll(connection)) {
        Box(Modifier.graphicsLayer { translationY = gap }) { content() }
        if (reading || gap > 0f) {
            Box(
                Modifier.fillMaxWidth().graphicsLayer { translationY = gap / 2f - with(density) { 15.dp.toPx() } },
                contentAlignment = Alignment.TopCenter,
            ) {
                Image(
                    painterResource(R.drawable.ic_mark), label,
                    Modifier.size(30.dp).graphicsLayer {
                        alpha = (gap / rest).coerceIn(0f, 1f) * pulse
                        rotationZ = when {
                            reduce -> 0f
                            pulled > 0f && !reading -> Pull.turn(pulled, threshold)
                            else -> spin.value
                        }
                    },
                    colorFilter = ColorFilter.tint(pal.t2),
                )
            }
        }
    }
}
