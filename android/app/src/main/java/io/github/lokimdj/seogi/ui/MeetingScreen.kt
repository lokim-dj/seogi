package io.github.lokimdj.seogi.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.lokimdj.seogi.R
import io.github.lokimdj.seogi.data.MeetingDoc
import java.time.ZoneId

/** Where the recording of an open meeting stands, for the playing line. */
sealed interface AudioState {
    /** In Drive, not on this phone yet: one tap downloads it. */
    data class InDrive(val sizeBytes: Long?) : AudioState
    data class Downloading(val progress: Float) : AudioState
    /** On this phone: ready to play. */
    data object Ready : AudioState
    data object Failed : AudioState
    /** Neither on this phone nor in Drive. */
    data object None : AudioState
}

/** What the playing line shows while the recording is ready. */
data class PlayState(val playing: Boolean, val positionMs: Long, val durationMs: Long)

/**
 * One meeting's minutes, the way the computer lays them out: the tasks
 * first, then the story, then everything else folded under "자세히", with a
 * thin playing line at the bottom. Every source is a timestamp key that plays
 * the recording from there; quotes are in 「 」.
 */
@Composable
fun MeetingScreen(
    title: String,
    startedAt: Long,
    durationMs: Long,
    fromPhone: Boolean,
    doc: MeetingDoc?,
    now: Long,
    audio: AudioState,
    play: PlayState,
    onBack: () -> Unit,
    onPlayAt: (Long?) -> Unit,
    onPause: () -> Unit,
) {
    val zone = ZoneId.systemDefault()
    var more by rememberSaveable { mutableStateOf(false) }
    Column(Modifier.fillMaxSize().padding(horizontal = Gutter)) {
        Spacer(Modifier.height(10.dp))
        BackBar(null, onBack)
        LazyColumn(Modifier.weight(1f)) {
            item {
                Column(Modifier.padding(top = 14.dp, bottom = 18.dp)) {
                    T(title, size = 20.sp)
                    val sub = listOf(
                        Format.day(startedAt, now, zone, stringResource(R.string.today), stringResource(R.string.yesterday)),
                        Format.length(durationMs),
                        stringResource(if (fromPhone) R.string.m_from_phone else R.string.m_from_computer),
                    ).joinToString(" · ")
                    T(sub, size = 12.5.sp, color = pal.t3, medium = true, modifier = Modifier.padding(top = 3.dp))
                    if (doc != null && doc.summary.isNotBlank()) {
                        T(doc.summary, size = 14.sp, color = pal.t2, medium = true, modifier = Modifier.padding(top = 12.dp))
                    }
                }
            }
            if (doc == null) {
                item { T(stringResource(R.string.m_not_read), size = 13.sp, color = pal.t3, medium = true) }
                return@LazyColumn
            }
            item { Label(stringResource(R.string.m_tasks)); Hairline() }
            if (doc.tasks.isEmpty()) {
                item {
                    T(stringResource(R.string.m_no_tasks), size = 13.sp, color = pal.t3, medium = true, modifier = Modifier.padding(vertical = 12.dp))
                    Hairline()
                }
            }
            items(doc.tasks) { t ->
                TaskLine(t, onPlayAt)
                Hairline()
            }
            if (doc.story.isNotEmpty()) {
                item { Spacer(Modifier.height(22.dp)); Label(stringResource(R.string.m_story)); Hairline() }
                items(doc.story) { part ->
                    Column(Modifier.fillMaxWidth().padding(vertical = 11.dp)) {
                        if (part.title.isNotBlank()) T(part.title, size = 14.5.sp)
                        T(part.text, size = 13.5.sp, color = pal.t2, medium = true, modifier = Modifier.padding(top = 3.dp))
                        Sources(part.sources, onPlayAt)
                    }
                    Hairline()
                }
            }
            item {
                T(
                    stringResource(if (more) R.string.m_less else R.string.m_more), size = 13.5.sp, color = pal.t3, medium = true,
                    modifier = Modifier.padding(top = 18.dp).clickable(role = Role.Button) { more = !more }.padding(vertical = 10.dp),
                )
            }
            if (more) {
                doc.otherSections.forEach { section ->
                    item { Spacer(Modifier.height(10.dp)); Label(section.title); Hairline() }
                    items(section.entries) { e ->
                        Column(Modifier.fillMaxWidth().padding(vertical = 10.dp)) {
                            T(e.text, size = 13.5.sp, medium = true)
                            if (e.needsCheck) T(stringResource(R.string.m_needs_check), size = 12.sp, color = pal.rt, medium = true)
                            Sources(e.sources, onPlayAt)
                        }
                        Hairline()
                    }
                }
                if (doc.marksMs.isNotEmpty()) {
                    item {
                        Spacer(Modifier.height(10.dp))
                        Label(stringResource(R.string.m_marks))
                        Hairline()
                        Row(Modifier.padding(vertical = 8.dp), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                            doc.marksMs.forEach { ms -> TimeKey(ms, onPlayAt) }
                        }
                        Hairline()
                    }
                }
                if (doc.lines.isNotEmpty()) {
                    item { Spacer(Modifier.height(10.dp)); Label(stringResource(R.string.m_transcript)); Hairline() }
                    items(doc.lines) { line ->
                        Row(Modifier.fillMaxWidth().padding(vertical = 7.dp)) {
                            TimeKey(line.startMs, onPlayAt)
                            Spacer(Modifier.width(10.dp))
                            Column(Modifier.weight(1f)) {
                                doc.speakerName(line.speaker)?.let { T(it, size = 11.5.sp, color = pal.t3, medium = true) }
                                T(line.text, size = 13.5.sp, medium = true)
                            }
                        }
                    }
                }
            }
            item { Spacer(Modifier.height(20.dp)) }
        }
        PlayLine(audio, play, onPlayAt, onPause)
    }
}

@Composable
private fun TaskLine(t: MeetingDoc.Entry, onPlayAt: (Long?) -> Unit) {
    Row(Modifier.fillMaxWidth().padding(vertical = 11.dp)) {
        Box(
            Modifier.padding(top = 2.dp).size(16.dp).clip(RoundedCornerShape(4.dp))
                .border(1.5.dp, if (t.done) pal.t3 else pal.t1, RoundedCornerShape(4.dp))
                .background(if (t.done) pal.t3 else Color.Transparent),
        )
        Spacer(Modifier.width(12.dp))
        Column(Modifier.weight(1f)) {
            T(t.text, size = 14.5.sp, color = if (t.done) pal.t3 else pal.t1, medium = t.done)
            val who = listOf(t.owner, t.due).filter { it.isNotBlank() }
            if (who.isNotEmpty()) T(who.joinToString(" · "), size = 12.sp, color = pal.t3, medium = true, modifier = Modifier.padding(top = 2.dp))
            if (t.needsCheck) T(stringResource(R.string.m_needs_check), size = 12.sp, color = pal.rt, medium = true, modifier = Modifier.padding(top = 2.dp))
            Sources(t.sources, onPlayAt)
        }
    }
}

/** Each source: its timestamp key and the short quote it rests on. */
@Composable
private fun Sources(sources: List<MeetingDoc.Source>, onPlayAt: (Long?) -> Unit) {
    sources.forEach { s ->
        Row(Modifier.padding(top = 4.dp), verticalAlignment = Alignment.Top) {
            TimeKey(s.ms, onPlayAt)
            if (s.quote.isNotBlank()) {
                Spacer(Modifier.width(8.dp))
                T("「${s.quote}」", size = 12.5.sp, color = pal.t2, medium = true, modifier = Modifier.weight(1f))
            }
        }
    }
}

/** `0:01:01`: plays the recording from there. */
@Composable
fun TimeKey(ms: Long, onPlayAt: (Long?) -> Unit) {
    T(
        Format.clock(ms), size = 12.sp, color = pal.t3, medium = true, tabular = true,
        modifier = Modifier.clickable(role = Role.Button) { onPlayAt(ms) },
    )
}

/** The thin playing line at the bottom. */
@Composable
private fun PlayLine(audio: AudioState, play: PlayState, onPlayAt: (Long?) -> Unit, onPause: () -> Unit) {
    Column(Modifier.fillMaxWidth().padding(top = 6.dp, bottom = 12.dp)) {
        val fraction = when (audio) {
            is AudioState.Downloading -> audio.progress
            AudioState.Ready -> if (play.durationMs > 0) play.positionMs.toFloat() / play.durationMs else 0f
            else -> 0f
        }
        Box(Modifier.fillMaxWidth().height(2.dp).background(pal.s2)) {
            Box(Modifier.fillMaxWidth(fraction.coerceIn(0f, 1f)).height(2.dp).background(pal.t1))
        }
        Row(Modifier.fillMaxWidth().padding(top = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            when (audio) {
                is AudioState.InDrive -> T(
                    stringResource(R.string.p_download, audio.sizeBytes?.let { mb(it) } ?: ""), size = 13.sp, color = pal.t2, medium = true,
                    modifier = Modifier.clickable(role = Role.Button) { onPlayAt(null) }.padding(vertical = 6.dp),
                )
                is AudioState.Downloading -> T(
                    stringResource(R.string.p_downloading, (audio.progress * 100).toInt()), size = 13.sp, color = pal.t3, medium = true,
                    modifier = Modifier.padding(vertical = 6.dp),
                )
                AudioState.Failed -> T(
                    stringResource(R.string.p_failed), size = 13.sp, color = pal.rt,
                    modifier = Modifier.clickable(role = Role.Button) { onPlayAt(null) }.padding(vertical = 6.dp),
                )
                AudioState.None -> T(stringResource(R.string.p_no_audio), size = 13.sp, color = pal.t3, medium = true, modifier = Modifier.padding(vertical = 6.dp))
                AudioState.Ready -> {
                    T(
                        stringResource(if (play.playing) R.string.p_pause else R.string.p_play), size = 14.sp,
                        modifier = Modifier.clickable(role = Role.Button) { if (play.playing) onPause() else onPlayAt(null) }
                            .padding(vertical = 6.dp, horizontal = 2.dp),
                    )
                    Spacer(Modifier.weight(1f))
                    T(
                        "${Format.clock(play.positionMs)} / ${Format.clock(play.durationMs)}", size = 12.sp, color = pal.t3,
                        medium = true, tabular = true,
                    )
                }
            }
        }
    }
}

private fun mb(bytes: Long): String = if (bytes >= 1024L * 1024 * 1024) {
    "%.1f GB".format(bytes / (1024.0 * 1024 * 1024))
} else {
    "${(bytes / (1024 * 1024)).coerceAtLeast(1)} MB"
}
