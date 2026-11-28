package io.github.lokimdj.seogi.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.em
import androidx.compose.ui.unit.sp
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import io.github.lokimdj.seogi.R
import io.github.lokimdj.seogi.dictation.A11yLog
import io.github.lokimdj.seogi.dictation.SetupState
import io.github.lokimdj.seogi.dictation.SetupStep

/** Where the model's download stands, as the screen shows it. */
sealed interface ModelDownload {
    data object Idle : ModelDownload
    data class WaitingForNetwork(val wifi: Boolean) : ModelDownload
    data class Running(val done: Long, val size: Long) : ModelDownload
    data object Wrong : ModelDownload
    data object Unavailable : ModelDownload
}

/**
 * Settings › 말로 입력: one step a screen (microphone and notifications, the
 * key over other apps, the accessibility step, the model), then the switch.
 * Each step says in plain words what it is for before anything is asked.
 */
@Composable
fun DictationScreen(
    state: SetupState,
    cpuSupported: Boolean,
    on: Boolean,
    download: ModelDownload,
    metered: Boolean,
    shortOfSpaceMb: Int,
    onBack: () -> Unit,
    onMic: () -> Unit,
    onOverlay: () -> Unit,
    onAccessibility: () -> Unit,
    onCopyOnly: () -> Unit,
    onGetModel: () -> Unit,
    onStopModel: () -> Unit,
    onSwitch: (Boolean) -> Unit,
    a11yProblem: Boolean = false,
    onFixA11y: () -> Unit = {},
    log: List<A11yLog.Entry> = emptyList(),
) {
    val step = state.current()
    Column(Modifier.fillMaxSize().padding(horizontal = Gutter)) {
        Spacer(Modifier.height(10.dp))
        BackBar(stringResource(R.string.dictation), onBack)
        Column(Modifier.weight(1f).verticalScroll(rememberScrollState())) {
            // Accessibility dropped after it was set up (#98): the same one tap as the notification, on top.
            if (a11yProblem && cpuSupported) {
                ListLine(
                    stringResource(R.string.a11y_dropped_line), stringResource(R.string.a11y_dropped_action),
                    stringResource(R.string.a11y_dropped_why), stateColor = pal.rt, onClick = onFixA11y,
                )
                Hairline()
            }
            when {
                !cpuSupported -> Sheet(stringResource(R.string.dict_cpu_title), null) {
                    Body(stringResource(R.string.dict_cpu_body))
                }
                step == SetupStep.MIC -> Sheet(stringResource(R.string.dict_mic_title), state.position(step)) {
                    Body(stringResource(R.string.dict_mic_body))
                    Keys { ArrowKey(stringResource(R.string.dict_mic_action), bright = true, onClick = onMic) }
                }
                step == SetupStep.OVERLAY -> Sheet(stringResource(R.string.dict_overlay_title), state.position(step)) {
                    Body(stringResource(R.string.dict_overlay_body))
                    Quiet(stringResource(R.string.dict_overlay_how))
                    Keys { ArrowKey(stringResource(R.string.dict_open_settings), bright = true, onClick = onOverlay) }
                }
                step == SetupStep.ACCESSIBILITY -> Sheet(stringResource(R.string.dict_a11y_title), state.position(step)) {
                    Body(stringResource(R.string.dict_a11y_body))
                    Body(stringResource(R.string.dict_a11y_promise))
                    Quiet(stringResource(R.string.dict_a11y_how))
                    Keys {
                        ArrowKey(stringResource(R.string.dict_a11y_agree), bright = true, onClick = onAccessibility)
                        T(
                            stringResource(R.string.dict_a11y_skip), size = 13.sp, color = pal.t3, medium = true,
                            modifier = Modifier.clickable(role = Role.Button, onClick = onCopyOnly).padding(vertical = 10.dp),
                        )
                    }
                }
                step == SetupStep.MODEL -> Sheet(stringResource(R.string.dict_model_title), state.position(step)) {
                    Body(stringResource(R.string.dict_model_body))
                    ModelPart(download, metered, shortOfSpaceMb, onGetModel, onStopModel)
                }
                else -> Ready(state, on, onSwitch, onAccessibility = {
                    if (state.copyOnly && !state.accessibility) onAccessibility()
                })
            }
            if (log.isNotEmpty()) DropLog(log)
        }
    }
}

/** The short drop log ([A11yLog]), newest first, so the next try on a phone can show why it went. */
@Composable
private fun DropLog(log: List<A11yLog.Entry>) {
    val zone = ZoneId.systemDefault()
    val at = DateTimeFormatter.ofPattern("M/d HH:mm")
    Column(Modifier.fillMaxWidth().padding(top = 26.dp, bottom = 20.dp)) {
        T(stringResource(R.string.a11y_log_title), size = 12.5.sp, color = pal.t3, medium = true)
        T(stringResource(R.string.a11y_log_why), size = 12.sp, color = pal.t3, medium = true, modifier = Modifier.padding(top = 2.dp, bottom = 6.dp))
        for (e in log.takeLast(10).asReversed()) {
            val time = at.format(Instant.ofEpochMilli(e.at).atZone(zone))
            T("$time  ${dropText(e)}", size = 12.5.sp, color = pal.t2, medium = true, modifier = Modifier.padding(vertical = 3.dp))
        }
    }
}

@Composable
private fun dropText(e: A11yLog.Entry): String = when (e.kind) {
    A11yLog.Kind.DROPPED -> stringResource(R.string.a11y_log_dropped)
    A11yLog.Kind.TURNED_OFF -> stringResource(R.string.a11y_log_turned_off)
    A11yLog.Kind.BACK -> stringResource(R.string.a11y_log_back)
    A11yLog.Kind.EXIT -> {
        val why = stringResource(
            when (e.detail.substringAfter(':')) {
                "low-memory" -> R.string.exit_low_memory
                "crash" -> R.string.exit_crash
                "native-crash" -> R.string.exit_native_crash
                "anr" -> R.string.exit_anr
                "force-stop" -> R.string.exit_force_stop
                "signaled" -> R.string.exit_signaled
                "excessive-use" -> R.string.exit_excessive_use
                "dependency-died" -> R.string.exit_dependency_died
                "init-failure" -> R.string.exit_init_failure
                "permission-change" -> R.string.exit_permission_change
                "freezer" -> R.string.exit_freezer
                "user-stopped" -> R.string.exit_user_stopped
                else -> R.string.exit_other
            },
        )
        stringResource(if (e.detail.startsWith("engine")) R.string.a11y_log_exit_engine else R.string.a11y_log_exit_dictation, why)
    }
}

@Composable
private fun Sheet(title: String, position: Pair<Int, Int>?, content: @Composable ColumnScope.() -> Unit) {
    Column(Modifier.fillMaxWidth().padding(top = 22.dp, bottom = 20.dp)) {
        if (position != null) {
            T(stringResource(R.string.dict_step, position.first, position.second), size = 12.5.sp, color = pal.t3, medium = true, tabular = true)
            Spacer(Modifier.height(8.dp))
        }
        T(title, size = 23.sp, letterSpacing = (-0.03).em, lineHeight = 1.25f)
        Spacer(Modifier.height(4.dp))
        content()
    }
}

@Composable
private fun Body(text: String) =
    T(text, size = 14.5.sp, color = pal.t2, medium = true, modifier = Modifier.padding(top = 12.dp))

@Composable
private fun Quiet(text: String, color: androidx.compose.ui.graphics.Color = pal.t3) =
    T(text, size = 12.5.sp, color = color, medium = true, modifier = Modifier.padding(top = 12.dp))

@Composable
private fun Keys(content: @Composable ColumnScope.() -> Unit) {
    Spacer(Modifier.height(18.dp))
    Hairline()
    Column(Modifier.padding(top = 8.dp), content = content)
}

@Composable
private fun ModelPart(download: ModelDownload, metered: Boolean, shortOfSpaceMb: Int, onGet: () -> Unit, onStop: () -> Unit) {
    when (download) {
        is ModelDownload.Running -> {
            val checking = download.done >= download.size
            val pct = (download.done * 100 / download.size.coerceAtLeast(1)).toInt()
            Spacer(Modifier.height(18.dp))
            T(
                if (checking) stringResource(R.string.dict_model_checking) else stringResource(R.string.dict_model_progress, pct),
                size = 14.sp, tabular = true,
            )
            Box(Modifier.padding(top = 8.dp).fillMaxWidth().height(2.dp).background(pal.s2)) {
                Box(Modifier.fillMaxWidth((download.done.toFloat() / download.size).coerceIn(0f, 1f)).height(2.dp).background(pal.t1))
            }
            Quiet(stringResource(R.string.dict_model_note))
            if (!checking) Keys { StopKey(onStop) }
        }
        is ModelDownload.WaitingForNetwork -> {
            Spacer(Modifier.height(18.dp))
            T(stringResource(if (download.wifi) R.string.dict_model_waiting_wifi else R.string.dict_model_waiting), size = 14.sp)
            Quiet(stringResource(R.string.dict_model_note))
            Keys { StopKey(onStop) }
        }
        ModelDownload.Idle, ModelDownload.Wrong, ModelDownload.Unavailable -> {
            if (download == ModelDownload.Wrong) Quiet(stringResource(R.string.dict_model_wrong), pal.rt)
            if (download == ModelDownload.Unavailable) Quiet(stringResource(R.string.dict_model_unavailable), pal.rt)
            if (metered) Quiet(stringResource(R.string.dict_model_metered), pal.rt)
            if (shortOfSpaceMb > 0) Quiet(stringResource(R.string.dict_model_space, shortOfSpaceMb), pal.rt)
            Keys {
                ArrowKey(stringResource(R.string.dict_model_get), bright = shortOfSpaceMb <= 0, onClick = onGet, enabled = shortOfSpaceMb <= 0)
            }
        }
    }
}

@Composable
private fun StopKey(onStop: () -> Unit) = T(
    stringResource(R.string.dict_model_stop), size = 14.sp, color = pal.t3, medium = true,
    modifier = Modifier.clickable(role = Role.Button, onClick = onStop).padding(vertical = 10.dp),
)

@Composable
private fun Ready(state: SetupState, on: Boolean, onSwitch: (Boolean) -> Unit, onAccessibility: () -> Unit) {
    Column(Modifier.fillMaxWidth().padding(top = 22.dp)) {
        T(stringResource(R.string.dict_ready_title), size = 23.sp, letterSpacing = (-0.03).em, lineHeight = 1.25f)
        Spacer(Modifier.height(18.dp))
        Hairline()
        val direct = state.accessibility
        SwitchLine(
            stringResource(R.string.dict_key), on,
            stringResource(if (direct) R.string.dict_key_why else R.string.dict_key_why_always),
            onChange = onSwitch,
        )
        Hairline()
        StepLine(
            stringResource(R.string.dict_insert),
            stringResource(if (direct) R.string.dict_insert_direct else R.string.dict_insert_copy),
            stringResource(if (direct) R.string.dict_insert_direct_why else R.string.dict_insert_copy_why),
            done = direct, onClick = onAccessibility,
        )
        Hairline()
        T(stringResource(R.string.dict_how), size = 13.sp, color = pal.t2, medium = true, modifier = Modifier.padding(top = 14.dp))
        T(stringResource(R.string.dict_privacy), size = 12.5.sp, color = pal.t3, medium = true, modifier = Modifier.padding(top = 12.dp))
    }
}
