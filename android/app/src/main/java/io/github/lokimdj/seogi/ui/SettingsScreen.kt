package io.github.lokimdj.seogi.ui

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import io.github.lokimdj.seogi.BuildConfig
import io.github.lokimdj.seogi.R

/** Settings: the account, the Wi-Fi rule, quality, calendar, dictation's place, and about. */
@Composable
fun SettingsScreen(
    account: String?,
    wifiOnly: Boolean,
    calendar: Boolean,
    onBack: () -> Unit,
    onLogin: () -> Unit,
    onWifiOnly: (Boolean) -> Unit,
    onCalendar: () -> Unit,
    onPrivacy: () -> Unit,
    onFeedback: () -> Unit,
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
                stringResource(R.string.dictation), stringResource(R.string.set_dictation_value),
                stringResource(R.string.set_dictation_why), done = true, onClick = {},
            )
            Hairline()
            StepLine(stringResource(R.string.set_privacy), "↗", "", done = false, onClick = onPrivacy)
            Hairline()
            StepLine(
                stringResource(R.string.set_feedback), "↗", stringResource(R.string.set_feedback_why),
                done = false, onClick = onFeedback,
            )
            Hairline()
            StepLine(stringResource(R.string.set_version), BuildConfig.VERSION_NAME, "", done = true, onClick = {})
            Hairline()
        }
    }
}
