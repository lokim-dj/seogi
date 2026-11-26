package io.github.lokimdj.seogi.ui

import androidx.compose.foundation.Image
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.ui.unit.Dp
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.ColorFilter
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.lokimdj.seogi.R
import io.github.lokimdj.seogi.data.Recording
import io.github.lokimdj.seogi.data.UploadState
import io.github.lokimdj.seogi.record.CalendarEvent
import io.github.lokimdj.seogi.record.RecState
import java.time.ZoneId

/**
 * Home, A: the symbol is the record key. One line for the next event above,
 * the last three recordings and their upload state below.
 */
@Composable
fun HomeScreen(
    rec: RecState,
    event: CalendarEvent?,
    now: Long,
    recordings: List<Recording>,
    wifiOnly: Boolean,
    signedIn: Boolean,
    onRecord: () -> Unit,
    onDictation: () -> Unit,
    onSettings: () -> Unit,
    onUploads: () -> Unit,
    onRetry: (Recording) -> Unit,
    onLogin: () -> Unit,
) {
    val zone = ZoneId.systemDefault()
    val eventLine: @Composable () -> Unit = {
        Column(Modifier.height(44.dp)) {
            if (event != null) {
                val running = event.begin <= now
                T(
                    stringResource(if (running) R.string.now_event else R.string.next_event, Format.hm(event.begin, zone)),
                    size = 12.5.sp, color = pal.t3, medium = true,
                )
                T(event.title, size = 16.sp, maxLines = 1, modifier = Modifier.padding(top = 2.dp))
            }
        }
    }
    val hint = when {
        rec is RecState.Recording -> R.string.record_hint_running
        rec is RecState.Saving -> R.string.record_hint_saving
        event != null -> R.string.record_hint_event
        else -> R.string.record_hint
    }
    val keyBlock: @Composable (Dp) -> Unit = { size ->
        Column(Modifier.fillMaxWidth(), horizontalAlignment = Alignment.CenterHorizontally) {
            RecordKey(rec, onRecord, size, Modifier.padding(top = 14.dp))
            T(
                stringResource(hint), size = 12.5.sp, color = pal.t3, medium = true, align = TextAlign.Center,
                modifier = Modifier.fillMaxWidth(),
            )
        }
    }
    val recent: @Composable () -> Unit = {
        Column {
            Label(stringResource(R.string.recent), stringResource(R.string.see_all), onUploads)
            Hairline()
            if (!signedIn) {
                ListLine(
                    stringResource(R.string.need_login_line), stringResource(R.string.login), null,
                    stateColor = pal.rt, onClick = onLogin,
                )
                Hairline()
            }
            if (recordings.isEmpty()) {
                T(
                    stringResource(R.string.no_recordings), size = 12.5.sp, color = pal.t3, medium = true,
                    modifier = Modifier.padding(vertical = 14.dp),
                )
                Hairline()
            }
            recordings.take(3).forEach { r ->
                val l = lineText(r, recordings, wifiOnly, now, detailed = false)
                ListLine(
                    r.title, l.state, l.sub, l.color, l.medium, l.progress,
                    onClick = if (r.upload == UploadState.FAILED) ({ onRetry(r) }) else onUploads,
                )
                Hairline()
            }
            Spacer(Modifier.height(10.dp))
        }
    }
    val bar: @Composable () -> Unit = {
        Spacer(Modifier.height(10.dp))
        BrandBar {
            Row(horizontalArrangement = Arrangement.spacedBy(22.dp)) {
                Icon(
                    R.drawable.ic_dictation, 22.dp, pal.t2, label = stringResource(R.string.dictation),
                    modifier = Modifier.clickable(role = Role.Button, onClick = onDictation),
                )
                Icon(
                    R.drawable.ic_settings, 22.dp, pal.t2, label = stringResource(R.string.settings),
                    modifier = Modifier.clickable(role = Role.Button, onClick = onSettings),
                )
            }
        }
    }

    BoxWithConstraints(Modifier.fillMaxSize().padding(horizontal = Gutter)) {
        val h = maxHeight
        if (isWide()) {
            // Sideways or unfolded: the key on the left, the recordings on the right.
            Column(Modifier.fillMaxSize()) {
                bar()
                Row(Modifier.weight(1f)) {
                    Column(Modifier.weight(1f).fillMaxHeight().verticalScroll(rememberScrollState())) {
                        Spacer(Modifier.height(10.dp))
                        eventLine()
                        keyBlock(minOf(240.dp, h * 0.42f))
                    }
                    Spacer(Modifier.width(40.dp))
                    Column(Modifier.weight(1f).fillMaxHeight().verticalScroll(rememberScrollState())) {
                        Spacer(Modifier.height(20.dp))
                        recent()
                    }
                }
            }
        } else {
            // Upright: one column that scrolls when the screen is short.
            Column(
                Modifier.fillMaxWidth().heightIn(min = maxHeight).verticalScroll(rememberScrollState()),
                verticalArrangement = Arrangement.SpaceBetween,
            ) {
                Column {
                    bar()
                    Spacer(Modifier.height(26.dp))
                    eventLine()
                    keyBlock(minOf(240.dp, h * 0.32f))
                }
                Column(Modifier.padding(top = 24.dp)) { recent() }
            }
        }
    }
}

/** The big tilted oval; a red dot inside while recording. */
@Composable
private fun RecordKey(rec: RecState, onClick: () -> Unit, size: Dp, modifier: Modifier = Modifier) {
    Box(
        modifier.size(size).clip(CircleShape).clickable(role = Role.Button, onClick = onClick),
        contentAlignment = Alignment.Center,
    ) {
        Image(
            painterResource(R.drawable.ic_mark), stringResource(R.string.record_hint),
            Modifier.fillMaxSize(), colorFilter = ColorFilter.tint(pal.t1),
        )
        if (rec !is RecState.Idle) RecDot(size * 0.17f, blink = rec is RecState.Recording)
    }
}
