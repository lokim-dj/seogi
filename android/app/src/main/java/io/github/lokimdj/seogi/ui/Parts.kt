package io.github.lokimdj.seogi.ui

import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.animateDpAsState
import androidx.compose.animation.core.snap
import androidx.compose.animation.core.tween
import androidx.compose.foundation.selection.toggleable
import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.keyframes
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.window.Dialog
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraintsScope
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

/** Sideways phone or an unfolded Fold: room for two panes side by side. */
fun BoxWithConstraintsScope.isWide(): Boolean = maxWidth > maxHeight && maxWidth >= 560.dp

/**
 * Screens that are one column of reading (first run, uploads, settings,
 * dictation) stay this wide on a big screen, centred.
 */
val ReadingWidth = 600.dp

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
fun BackBar(title: String?, onBack: () -> Unit, action: String? = null, onAction: () -> Unit = {}) {
    Row(Modifier.fillMaxWidth().height(48.dp), verticalAlignment = Alignment.CenterVertically) {
        Row(
            Modifier.clickable(role = Role.Button, onClick = onBack).padding(vertical = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(R.drawable.ic_back, 16.dp, pal.t3)
            Spacer(Modifier.width(8.dp))
            T(stringResource(R.string.home), size = 13.5.sp, color = pal.t3, quiet = true)
        }
        Spacer(Modifier.weight(1f))
        if (title != null) T(title, size = 12.5.sp, color = pal.t3, quiet = true)
        if (action != null) {
            T(
                action, size = 13.5.sp, color = pal.t3, quiet = true,
                modifier = Modifier.clickable(role = Role.Button, onClick = onAction).padding(vertical = 10.dp, horizontal = 4.dp),
            )
        }
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
        T(text, size = 16.sp, color = color, quiet = !bright)
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
@OptIn(ExperimentalFoundationApi::class)
@Composable
fun ListLine(
    name: String,
    state: String,
    sub: String?,
    stateColor: Color = pal.t1,
    stateQuiet: Boolean = false,
    progress: Float? = null,
    onClick: (() -> Unit)? = null,
    onLongClick: (() -> Unit)? = null,
    longClickLabel: String? = null,
) {
    Column(
        Modifier.fillMaxWidth()
            .then(
                if (onClick != null || onLongClick != null) {
                    Modifier.combinedClickable(onClick = onClick ?: {}, onLongClick = onLongClick, onLongClickLabel = longClickLabel)
                } else {
                    Modifier
                },
            )
            .padding(vertical = 11.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            T(name, size = 14.5.sp, maxLines = 1, modifier = Modifier.weight(1f))
            Spacer(Modifier.width(12.dp))
            T(state, size = 12.5.sp, color = stateColor, quiet = stateQuiet, maxLines = 1)
        }
        if (sub != null) T(sub, size = 12.sp, color = pal.t3, quiet = true, modifier = Modifier.padding(top = 2.dp))
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
            if (tag != null) T(tag, size = 12.5.sp, color = pal.t3, quiet = true)
            Spacer(Modifier.weight(1f))
            T(action, size = 13.5.sp, color = if (done) pal.t3 else pal.t1, quiet = done, maxLines = 1)
        }
        if (why.isNotEmpty()) {
            T(why, size = 12.5.sp, color = pal.t3, quiet = true, modifier = Modifier.padding(top = 3.dp, end = 40.dp))
        }
    }
}

/**
 * A setting that is on or off: its name (and a line on what it does) beside a
 * switch, the whole line one tap target. Off: knob left on a grey track; on:
 * knob right on Seogi's red.
 */
@Composable
fun SwitchLine(name: String, on: Boolean, why: String, onChange: (Boolean) -> Unit) {
    Row(
        Modifier.fillMaxWidth()
            .toggleable(value = on, role = Role.Switch, onValueChange = onChange)
            .padding(vertical = 13.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f).padding(end = 16.dp)) {
            T(name, size = 15.sp)
            if (why.isNotEmpty()) T(why, size = 12.5.sp, color = pal.t3, quiet = true, modifier = Modifier.padding(top = 3.dp))
        }
        Switch(on)
    }
}

/** The switch itself, drawn to the design: 52 × 30 dp, a 24 dp knob. */
@Composable
private fun Switch(on: Boolean) {
    val still = LocalReduceMotion.current
    val knobAt by animateDpAsState(if (on) 25.dp else 3.dp, if (still) snap() else tween(160), label = "knob")
    val track by animateColorAsState(
        if (on) pal.red else pal.t3.copy(alpha = 0.45f), if (still) snap() else tween(160), label = "track",
    )
    Box(Modifier.size(width = 52.dp, height = 30.dp).clip(CircleShape).background(track)) {
        Box(
            Modifier.padding(start = knobAt, top = 3.dp).size(24.dp).clip(CircleShape).background(Color.White),
        )
    }
}

/**
 * A quiet in-app question: a title, a line, an optional check, and two text
 * keys. Used before anything that cannot be undone later.
 */
@Composable
fun ConfirmDialog(
    title: String,
    body: String,
    confirm: String,
    cancel: String,
    onConfirm: (Boolean) -> Unit,
    onDismiss: () -> Unit,
    check: String? = null,
    checkedAtFirst: Boolean = true,
) {
    var checked by remember { mutableStateOf(checkedAtFirst) }
    Dialog(onDismissRequest = onDismiss) {
        Column(
            Modifier.fillMaxWidth().clip(RoundedCornerShape(14.dp)).background(pal.s1)
                .padding(start = 22.dp, end = 22.dp, top = 20.dp, bottom = 8.dp),
        ) {
            T(title, size = 16.sp)
            T(body, size = 13.sp, color = pal.t2, quiet = true, modifier = Modifier.padding(top = 6.dp))
            if (check != null) {
                Row(
                    Modifier.padding(top = 16.dp).clickable(role = Role.Checkbox) { checked = !checked },
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Box(
                        Modifier.size(18.dp).clip(RoundedCornerShape(4.dp))
                            .border(1.5.dp, pal.t1, RoundedCornerShape(4.dp))
                            .background(if (checked) pal.t1 else Color.Transparent),
                    )
                    Spacer(Modifier.width(10.dp))
                    T(check, size = 13.sp)
                }
            }
            Row(Modifier.fillMaxWidth().padding(top = 14.dp), horizontalArrangement = Arrangement.End) {
                T(
                    cancel, size = 14.sp, color = pal.t3, quiet = true,
                    modifier = Modifier.clickable(role = Role.Button, onClick = onDismiss).padding(12.dp),
                )
                T(
                    confirm, size = 14.sp, color = pal.rt,
                    modifier = Modifier.clickable(role = Role.Button) { onConfirm(checked) }.padding(12.dp),
                )
            }
        }
    }
}

/** A quiet in-app question that asks for words: a title, a text field, two keys. */
@Composable
fun TextDialog(
    title: String,
    initial: String,
    confirm: String,
    cancel: String,
    onConfirm: (String) -> Unit,
    onDismiss: () -> Unit,
) {
    var text by remember { mutableStateOf(initial) }
    Dialog(onDismissRequest = onDismiss) {
        Column(
            Modifier.fillMaxWidth().clip(RoundedCornerShape(14.dp)).background(pal.s1)
                .padding(start = 22.dp, end = 22.dp, top = 20.dp, bottom = 8.dp),
        ) {
            T(title, size = 16.sp)
            BasicTextField(
                value = text,
                onValueChange = { text = it },
                textStyle = TextStyle(fontFamily = Pretendard, fontWeight = FontWeight.Medium, fontSize = 15.sp, color = pal.t1),
                cursorBrush = SolidColor(pal.t1),
                modifier = Modifier.fillMaxWidth().padding(top = 14.dp)
                    .border(1.dp, pal.line, RoundedCornerShape(8.dp)).padding(12.dp),
            )
            Row(Modifier.fillMaxWidth().padding(top = 10.dp), horizontalArrangement = Arrangement.End) {
                T(
                    cancel, size = 14.sp, color = pal.t3, quiet = true,
                    modifier = Modifier.clickable(role = Role.Button, onClick = onDismiss).padding(12.dp),
                )
                T(
                    confirm, size = 14.sp,
                    modifier = Modifier.clickable(role = Role.Button) { onConfirm(text.trim()) }.padding(12.dp),
                )
            }
        }
    }
}

/** The bar at the bottom after a delete: what happened, and "되돌리기" for a few seconds. */
@Composable
fun UndoBar(text: String, action: String, onAction: () -> Unit, modifier: Modifier = Modifier) {
    Row(
        modifier.fillMaxWidth().padding(horizontal = 14.dp, vertical = 12.dp)
            .clip(RoundedCornerShape(14.dp)).background(pal.s2).padding(start = 18.dp, end = 6.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        T(text, size = 14.sp, modifier = Modifier.weight(1f).padding(vertical = 14.dp))
        T(
            action, size = 14.sp, color = pal.rt,
            modifier = Modifier.clickable(role = Role.Button, onClick = onAction).padding(12.dp),
        )
    }
}
