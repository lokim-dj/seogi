package io.github.lokimdj.seogi.ui

import android.os.SystemClock
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.em
import androidx.compose.ui.unit.sp
import io.github.lokimdj.seogi.R
import io.github.lokimdj.seogi.record.RecState
import io.github.lokimdj.seogi.record.RecorderService
import kotlinx.coroutines.delay

/**
 * Recording: a big running time, thin level bars, the important marks, and
 * two keys. Stopping takes a long press so a pocket tap cannot end a meeting.
 */
@OptIn(ExperimentalFoundationApi::class)
@Composable
fun RecordingScreen(rec: RecState, onBack: () -> Unit, onMark: () -> Unit, onStop: () -> Unit, onDiscard: () -> Unit = {}) {
    var askDiscard by remember { mutableStateOf(false) }
    if (askDiscard) {
        ConfirmDialog(
            title = stringResource(R.string.discard_title),
            body = stringResource(R.string.discard_body),
            confirm = stringResource(R.string.discard),
            cancel = stringResource(R.string.keep_recording),
            onConfirm = {
                askDiscard = false
                onDiscard()
            },
            onDismiss = { askDiscard = false },
        )
    }
    val r = rec as? RecState.Recording
    var elapsed by remember { mutableLongStateOf(0L) }
    val levels = remember { mutableStateListOf<Float>().apply { repeat(BARS) { add(0f) } } }
    LaunchedEffect(r?.id) {
        while (r != null) {
            elapsed = SystemClock.elapsedRealtime() - r.startedElapsed
            levels.removeAt(0)
            levels.add(RecorderService.level.value)
            delay(120)
        }
    }
    val haptic = LocalHapticFeedback.current

    if (r == null) {
        Column(Modifier.fillMaxSize().padding(horizontal = Gutter)) {
            Spacer(Modifier.height(10.dp))
            BackBar(null, onBack)
            Spacer(Modifier.height(30.dp))
            T(stringResource(R.string.saving), size = 18.sp, color = pal.t3, medium = true)
        }
        return
    }
    val head: @Composable () -> Unit = {
        Column {
            Row(Modifier.padding(top = 30.dp), verticalAlignment = Alignment.CenterVertically) {
                RecDot(9.dp)
                Spacer(Modifier.width(9.dp))
                T(stringResource(R.string.recording_now), size = 12.5.sp, color = pal.rt)
                if (r.fromCalendar) T(" · " + stringResource(R.string.from_calendar), size = 12.5.sp, color = pal.t3, medium = true)
            }
            T(r.title, size = 18.sp, maxLines = 2, modifier = Modifier.padding(top = 6.dp))
            T(
                Format.clock(elapsed), size = 54.sp, letterSpacing = (-0.04).em, lineHeight = 1f, tabular = true,
                modifier = Modifier.padding(top = 34.dp),
            )
            LevelBars(levels, Modifier.padding(top = 22.dp, bottom = 8.dp))
            T(stringResource(R.string.recording_note), size = 11.5.sp, color = pal.t3, medium = true)
        }
    }
    val marks: @Composable (Modifier) -> Unit = { modifier ->
        Column(modifier) {
            Label(stringResource(R.string.marks_count, r.marksMs.size))
            LazyColumn(Modifier.weight(1f)) {
                itemsIndexed(r.marksMs) { _, ms ->
                    Hairline()
                    ListLine(stringResource(R.string.mark), Format.mark(ms), null, pal.t3, stateMedium = true)
                }
                if (r.marksMs.isNotEmpty()) item { Hairline() }
            }
        }
    }
    val keys: @Composable () -> Unit = {
        Row(Modifier.fillMaxWidth().padding(top = 16.dp, bottom = 18.dp)) {
            RingKey(
                R.drawable.ic_bookmark, stringResource(R.string.action_mark), null,
                Modifier.weight(1f).combinedClickable(role = Role.Button, onClick = {
                    haptic.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                    onMark()
                }),
            )
            RingKey(
                R.drawable.ic_stop, stringResource(R.string.action_stop), stringResource(R.string.stop_long_press),
                Modifier.weight(1f).combinedClickable(
                    role = Role.Button,
                    onClick = {},
                    onLongClickLabel = stringResource(R.string.action_stop),
                    onLongClick = {
                        haptic.performHapticFeedback(HapticFeedbackType.LongPress)
                        onStop()
                    },
                ),
            )
        }
    }

    BoxWithConstraints(Modifier.fillMaxSize().padding(horizontal = Gutter)) {
        if (isWide()) {
            // Sideways or unfolded: time on the left, marks and keys on the right,
            // so the keys never fall off the screen.
            Column(Modifier.fillMaxSize()) {
                Spacer(Modifier.height(10.dp))
                BackBar(null, onBack, stringResource(R.string.discard)) { askDiscard = true }
                Row(Modifier.weight(1f)) {
                    Column(Modifier.weight(1f).fillMaxHeight().verticalScroll(rememberScrollState())) { head() }
                    Spacer(Modifier.width(40.dp))
                    Column(Modifier.weight(1f).fillMaxHeight()) {
                        Spacer(Modifier.height(24.dp))
                        marks(Modifier.weight(1f))
                        keys()
                    }
                }
            }
        } else {
            Column(Modifier.fillMaxSize()) {
                Spacer(Modifier.height(10.dp))
                BackBar(null, onBack, stringResource(R.string.discard)) { askDiscard = true }
                head()
                Spacer(Modifier.height(30.dp))
                marks(Modifier.weight(1f).heightIn(min = 60.dp))
                keys()
            }
        }
    }
}

@Composable
private fun RingKey(icon: Int, label: String, hint: String?, modifier: Modifier) {
    Column(modifier, horizontalAlignment = Alignment.CenterHorizontally) {
        Box(
            Modifier.size(76.dp).clip(CircleShape).border(1.5.dp, pal.t1, CircleShape),
            contentAlignment = Alignment.Center,
        ) { Icon(icon, 28.dp, pal.t1) }
        T(label, size = 13.5.sp, align = TextAlign.Center, modifier = Modifier.padding(top = 8.dp))
        if (hint != null) T(hint, size = 11.5.sp, color = pal.t3, medium = true)
    }
}

/** Older bars quieter, the newest few bright. */
@Composable
private fun LevelBars(levels: List<Float>, modifier: Modifier) {
    Row(
        modifier.fillMaxWidth().height(54.dp),
        horizontalArrangement = Arrangement.spacedBy(2.4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        levels.forEachIndexed { i, v ->
            val shown = kotlin.math.sqrt(v).coerceIn(0.04f, 1f)
            Box(
                Modifier.weight(1f).fillMaxHeight(shown).clip(RoundedCornerShape(1.dp))
                    .background(if (i >= levels.size - 6) pal.t1 else pal.t3),
            )
        }
    }
}

private const val BARS = 40
