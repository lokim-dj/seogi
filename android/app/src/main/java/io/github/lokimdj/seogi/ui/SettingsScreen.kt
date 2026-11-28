package io.github.lokimdj.seogi.ui

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
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
import androidx.compose.ui.unit.sp
import io.github.lokimdj.seogi.BuildConfig
import io.github.lokimdj.seogi.R

/**
 * Settings: the account, the Wi-Fi rule, quality, calendar, dictation (말로 입력),
 * about, and last where to get Seogi for the computer.
 */
@Composable
fun SettingsScreen(
    account: String?,
    wifiOnly: Boolean,
    calendar: Boolean,
    onBack: () -> Unit,
    onLogin: () -> Unit,
    loginMessage: String?,
    onWifiOnly: (Boolean) -> Unit,
    onCalendar: () -> Unit,
    onPrivacy: () -> Unit,
    onFeedback: () -> Unit,
    onLicenses: () -> Unit = {},
    dictationState: String = "",
    onDictation: () -> Unit = {},
    onComputerApp: () -> Unit = {},
    onCopyComputerApp: () -> Unit = {},
) {
    Column(Modifier.fillMaxSize().padding(horizontal = Gutter)) {
        Spacer(Modifier.height(10.dp))
        BackBar(stringResource(R.string.settings), onBack)
        Column(Modifier.weight(1f).verticalScroll(rememberScrollState()).padding(top = 14.dp)) {
            Hairline()
            StepLine(
                stringResource(R.string.set_account),
                if (account != null) stringResource(R.string.set_other_account) else stringResource(R.string.login),
                (if (account != null) account.ifEmpty { "Google" } + " · " else "") + stringResource(R.string.set_account_why),
                done = false, onClick = onLogin,
            )
            if (loginMessage != null) {
                T(loginMessage, size = 12.5.sp, color = pal.rt, modifier = Modifier.padding(bottom = 12.dp))
            }
            Hairline()
            StepLine(
                stringResource(R.string.wifi_only), stringResource(if (wifiOnly) R.string.on else R.string.off),
                stringResource(R.string.set_wifi_why), done = false, onClick = { onWifiOnly(!wifiOnly) },
            )
            Hairline()
            StepLine(
                stringResource(R.string.set_quality), stringResource(R.string.set_quality_value),
                stringResource(R.string.set_quality_why), done = true, onClick = {},
            )
            Hairline()
            StepLine(
                stringResource(R.string.onb_cal), stringResource(if (calendar) R.string.allowed else R.string.allow),
                stringResource(R.string.set_calendar_why), done = calendar, onClick = onCalendar,
            )
            Hairline()
            StepLine(
                stringResource(R.string.dictation), dictationState,
                stringResource(R.string.set_dictation_why), done = false, onClick = onDictation,
            )
            Hairline()
            StepLine(stringResource(R.string.set_privacy), "↗", "", done = false, onClick = onPrivacy)
            Hairline()
            StepLine(
                stringResource(R.string.set_feedback), "↗", stringResource(R.string.set_feedback_why),
                done = false, onClick = onFeedback,
            )
            Hairline()
            StepLine(stringResource(R.string.set_licenses), "›", "", done = false, onClick = onLicenses)
            Hairline()
            StepLine(stringResource(R.string.set_version), BuildConfig.VERSION_NAME, "", done = true, onClick = {})
            Hairline()
            ComputerAppLine(onComputerApp, onCopyComputerApp)
            Hairline()
        }
    }
}

/**
 * Where to get Seogi for the computer: a tap opens the download page, a long
 * press or "링크 복사" copies its address, to send to the computer.
 */
@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun ComputerAppLine(onOpen: () -> Unit, onCopy: () -> Unit) {
    val copy = stringResource(R.string.set_copy_link)
    Column(
        Modifier.fillMaxWidth()
            .combinedClickable(onClick = onOpen, onLongClick = onCopy, onLongClickLabel = copy)
            .padding(top = 13.dp, bottom = 5.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            T(stringResource(R.string.set_computer_app), size = 15.sp)
            Spacer(Modifier.weight(1f))
            T("↗", size = 13.5.sp, color = pal.t1, maxLines = 1)
        }
        T(
            stringResource(R.string.set_computer_app_why), size = 12.5.sp, color = pal.t3, quiet = true,
            modifier = Modifier.padding(top = 3.dp, end = 40.dp),
        )
        T(
            copy, size = 12.5.sp, color = pal.t2,
            modifier = Modifier.padding(top = 2.dp).clickable(onClick = onCopy).padding(top = 6.dp, bottom = 8.dp, end = 16.dp),
        )
    }
}
