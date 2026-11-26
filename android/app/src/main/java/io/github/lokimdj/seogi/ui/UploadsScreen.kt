package io.github.lokimdj.seogi.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.lokimdj.seogi.R
import io.github.lokimdj.seogi.data.Recording
import io.github.lokimdj.seogi.data.UploadState

/** Uploads: where they go, one state per line, and the Wi-Fi rule. */
@Composable
fun UploadsScreen(
    recordings: List<Recording>,
    account: String?,
    wifiOnly: Boolean,
    now: Long,
    onBack: () -> Unit,
    onRetry: (Recording) -> Unit,
    onOpenFolder: () -> Unit,
    onLogin: () -> Unit,
    onWifiOnly: (Boolean) -> Unit,
) {
    Column(Modifier.fillMaxSize().padding(horizontal = Gutter)) {
        Spacer(Modifier.height(10.dp))
        BackBar(stringResource(R.string.uploads), onBack)
        T(stringResource(R.string.upload_dest), size = 12.5.sp, color = pal.t3, medium = true, modifier = Modifier.padding(top = 22.dp))
        T(
            stringResource(R.string.upload_dest_path), size = 17.sp,
            modifier = Modifier.padding(top = 2.dp).clickable(enabled = account != null, onClick = onOpenFolder),
        )
        if (account != null) {
            T(stringResource(R.string.upload_dest_account, account.ifEmpty { "Google" }), size = 12.5.sp, color = pal.t3, medium = true)
        } else {
            T(
                stringResource(R.string.upload_not_logged_in), size = 12.5.sp, color = pal.rt,
                modifier = Modifier.clickable(onClick = onLogin),
            )
        }
        Spacer(Modifier.height(22.dp))
        LazyColumn(Modifier.weight(1f)) {
            items(recordings, key = { it.id }) { r ->
                val l = lineText(r, recordings, wifiOnly, now, detailed = true)
                Hairline()
                ListLine(
                    r.title, l.state, l.sub, l.color, l.medium, l.progress,
                    onClick = if (r.upload == UploadState.FAILED) ({ onRetry(r) }) else null,
                )
            }
            if (recordings.isNotEmpty()) item { Hairline() }
        }
        Column(Modifier.padding(top = 14.dp, bottom = 12.dp)) {
            Row(Modifier.fillMaxWidth().clickable { onWifiOnly(!wifiOnly) }.padding(bottom = 8.dp)) {
                T(stringResource(R.string.wifi_only), size = 14.sp, modifier = Modifier.weight(1f))
                T(stringResource(if (wifiOnly) R.string.on else R.string.off), size = 14.sp, color = if (wifiOnly) pal.t1 else pal.t3, medium = !wifiOnly)
            }
            T(stringResource(R.string.originals_stay), size = 11.5.sp, color = pal.t3, medium = true)
        }
    }
}
