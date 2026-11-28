package io.github.lokimdj.seogi.ui

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.em
import androidx.compose.ui.unit.sp
import io.github.lokimdj.seogi.R

/** What the phone has allowed so far. */
data class Perms(val mic: Boolean, val notifications: Boolean, val calendar: Boolean)

/**
 * First run, A: the four questions on one sheet, each with one line on why.
 * "시작하기" works once the microphone and notifications are allowed and is
 * bright when Drive is connected too; the calendar is optional.
 */
@Composable
fun OnboardingScreen(
    perms: Perms,
    account: String?,
    onMic: () -> Unit,
    onNotifications: () -> Unit,
    onCalendar: () -> Unit,
    onLogin: () -> Unit,
    loginMessage: String?,
    onStart: () -> Unit,
) {
    val signedIn = account != null
    val done = listOf(perms.mic, perms.notifications, perms.calendar, signedIn).count { it }
    val canStart = perms.mic && perms.notifications
    Column(Modifier.fillMaxSize().padding(horizontal = Gutter)) {
        Column(Modifier.weight(1f).verticalScroll(rememberScrollState())) {
            Spacer(Modifier.height(10.dp))
            BrandBar()
            Spacer(Modifier.height(22.dp))
            T(stringResource(R.string.onb_title), size = 23.sp, title = true, letterSpacing = (-0.025).em, lineHeight = 1.3f)
            T(
                stringResource(R.string.onb_lead), size = 12.5.sp, color = pal.t3, quiet = true,
                modifier = Modifier.padding(top = 10.dp, bottom = 22.dp),
            )
            Hairline()
            StepLine(stringResource(R.string.onb_mic), allowWord(perms.mic), stringResource(R.string.onb_mic_why), perms.mic, onMic)
            Hairline()
            StepLine(
                stringResource(R.string.onb_notif), allowWord(perms.notifications),
                stringResource(R.string.onb_notif_why), perms.notifications, onNotifications,
            )
            Hairline()
            StepLine(
                stringResource(R.string.onb_cal), allowWord(perms.calendar), stringResource(R.string.onb_cal_why),
                perms.calendar, onCalendar, tag = stringResource(R.string.onb_optional),
            )
            Hairline()
            StepLine(
                stringResource(R.string.onb_drive),
                if (signedIn) account!!.ifEmpty { stringResource(R.string.allowed) } else stringResource(R.string.login),
                stringResource(R.string.onb_drive_why), signedIn, onLogin,
            )
            if (loginMessage != null) {
                T(loginMessage, size = 12.5.sp, color = pal.rt, modifier = Modifier.padding(bottom = 12.dp))
            }
            Hairline()
            T(
                stringResource(R.string.onb_dictation_later), size = 11.5.sp, color = pal.t3, quiet = true,
                modifier = Modifier.padding(top = 16.dp),
            )
        }
        Row(Modifier.fillMaxWidth().padding(top = 18.dp, bottom = 14.dp), verticalAlignment = Alignment.CenterVertically) {
            T(stringResource(R.string.onb_done_count, done), size = 12.5.sp, color = pal.t3, quiet = true)
            Spacer(Modifier.weight(1f))
            ArrowKey(stringResource(R.string.onb_start), bright = canStart && signedIn, onClick = onStart, enabled = canStart)
        }
    }
}

@Composable
private fun allowWord(ok: Boolean) = stringResource(if (ok) R.string.allowed else R.string.allow)
