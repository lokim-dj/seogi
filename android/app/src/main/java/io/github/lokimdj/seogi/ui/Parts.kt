package io.github.lokimdj.seogi.ui

import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.keyframes
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ColorFilter
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.lokimdj.seogi.R

/** The side gutter every screen keeps (26 dp in the design). */
val Gutter = 26.dp

@Composable
fun Hairline(modifier: Modifier = Modifier) =
    Box(modifier.fillMaxWidth().height(1.dp).background(pal.line))

@Composable
fun Icon(res: Int, size: Dp, tint: Color, modifier: Modifier = Modifier, label: String? = null) =
    Image(painterResource(res), label, modifier.size(size), colorFilter = ColorFilter.tint(tint))

/** The top bar of the home and the first run: the symbol and the name. */
@Composable
fun BrandBar(right: @Composable () -> Unit = {}) {
    Row(Modifier.fillMaxWidth().height(48.dp), verticalAlignment = Alignment.CenterVertically) {
        Icon(R.drawable.ic_mark, 20.dp, pal.t1)
        Spacer(Modifier.width(7.dp))
        T(stringResource(R.string.app_name), size = 16.sp)
        Spacer(Modifier.weight(1f))
        right()
    }
}

/** The top bar of the inner screens: "← 홈" and a quiet name on the right. */
@Composable
fun BackBar(title: String?, onBack: () -> Unit) {
    Row(Modifier.fillMaxWidth().height(48.dp), verticalAlignment = Alignment.CenterVertically) {
        Row(
            Modifier.clickable(role = Role.Button, onClick = onBack).padding(vertical = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(R.drawable.ic_back, 16.dp, pal.t3)
            Spacer(Modifier.width(8.dp))
            T(stringResource(R.string.home), size = 13.5.sp, color = pal.t3, medium = true)
        }
        Spacer(Modifier.weight(1f))
        if (title != null) T(title, size = 12.5.sp, color = pal.t3, medium = true)
    }
}

/** A text key with an arrow, like "시작하기 →". */
@Composable
fun ArrowKey(text: String, bright: Boolean, onClick: () -> Unit, enabled: Boolean = true) {
    val color = if (bright) pal.t1 else pal.t3
    Row(
        Modifier.clickable(enabled = enabled, role = Role.Button, onClick = onClick).padding(vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        T(text, size = 16.sp, color = color, medium = !bright)
        Spacer(Modifier.width(8.dp))
        Icon(R.drawable.ic_arrow, 16.dp, color)
    }
}

/** The small grey label above a list, with an optional link on the right. */
@Composable
fun Label(text: String, link: String? = null, onLink: () -> Unit = {}) {
    Row(Modifier.fillMaxWidth().padding(bottom = 6.dp)) {
        T(text, size = 12.sp, color = pal.t3)
        Spacer(Modifier.weight(1f))
        if (link != null) T(link, size = 12.sp, color = pal.t3, modifier = Modifier.clickable(onClick = onLink))
    }
}

/**
 * One line of a list: a name, a state on the right, a quiet line under it and
 * an optional thin progress line, between hairlines.
 */
@Composable
fun ListLine(
    name: String,
    state: String,
    sub: String?,
    stateColor: Color = pal.t1,
    stateMedium: Boolean = false,
    progress: Float? = null,
    onClick: (() -> Unit)? = null,
) {
    Column(
        Modifier.fillMaxWidth()
            .then(if (onClick != null) Modifier.clickable(onClick = onClick) else Modifier)
            .padding(vertical = 11.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            T(name, size = 14.5.sp, maxLines = 1, modifier = Modifier.weight(1f))
            Spacer(Modifier.width(12.dp))
            T(state, size = 12.5.sp, color = stateColor, medium = stateMedium, maxLines = 1)
        }
        if (sub != null) T(sub, size = 12.sp, color = pal.t3, medium = true, modifier = Modifier.padding(top = 2.dp))
        if (progress != null) {
            Box(Modifier.padding(top = 5.dp).fillMaxWidth().height(2.dp).background(pal.s2)) {
                Box(Modifier.fillMaxWidth(progress.coerceIn(0f, 1f)).height(2.dp).background(pal.t1))
            }
        }
    }
}

/** The red dot: 1.6 s blink while recording, still when motion is reduced. */
@Composable
fun RecDot(size: Dp, modifier: Modifier = Modifier, blink: Boolean = true) {
    val alpha = if (!blink || LocalReduceMotion.current) {
        1f
    } else {
        val t = rememberInfiniteTransition(label = "rec")
        val a by t.animateFloat(
            initialValue = 1f,
            targetValue = 1f,
            animationSpec = infiniteRepeatable(
                keyframes {
                    durationMillis = 1600
                    1f at 880 using LinearEasing
                    0.18f at 1152
                    0.18f at 1408
                    1f at 1600
                },
                RepeatMode.Restart,
            ),
            label = "rec-alpha",
        )
        a
    }
    Box(modifier.size(size).alpha(alpha).clip(CircleShape).background(pal.red))
}

/** A step or setting: its name, an action or value on the right, and why (may be empty). */
@Composable
fun StepLine(name: String, action: String, why: String, done: Boolean, onClick: () -> Unit, tag: String? = null) {
    Column(Modifier.fillMaxWidth().clickable(enabled = !done, onClick = onClick).padding(vertical = 13.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            T(name, size = 15.sp)
            if (tag != null) T(tag, size = 12.5.sp, color = pal.t3, medium = true)
            Spacer(Modifier.weight(1f))
            T(action, size = 13.5.sp, color = if (done) pal.t3 else pal.t1, medium = done, maxLines = 1)
        }
        if (why.isNotEmpty()) {
            T(why, size = 12.5.sp, color = pal.t3, medium = true, modifier = Modifier.padding(top = 3.dp, end = 40.dp))
        }
    }
}
