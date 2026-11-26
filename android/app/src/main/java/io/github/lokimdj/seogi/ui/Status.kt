package io.github.lokimdj.seogi.ui

import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.stringResource
import io.github.lokimdj.seogi.R
import io.github.lokimdj.seogi.data.Recording
import io.github.lokimdj.seogi.data.UploadError
import io.github.lokimdj.seogi.data.UploadState
import java.time.ZoneId

/** How one recording's upload reads on a list line. */
data class LineText(val state: String, val color: Color, val medium: Boolean, val sub: String, val progress: Float?)

@Composable
private fun reason(e: UploadError?) = stringResource(
    when (e) {
        UploadError.OFFLINE -> R.string.err_offline
        UploadError.NEED_LOGIN -> R.string.err_need_login
        UploadError.MISSING_ON_PHONE -> R.string.err_missing
        UploadError.SERVER, null -> R.string.err_server
    },
)

/** [detailed] is the uploads screen: "지금 다시" and the longer reasons. */
@Composable
fun lineText(r: Recording, all: List<Recording>, wifiOnly: Boolean, now: Long, detailed: Boolean): LineText {
    val p = pal
    val zone = ZoneId.systemDefault()
    val base = buildList {
        add(Format.day(r.startedAt, now, zone, stringResource(R.string.today), stringResource(R.string.yesterday)))
        add(Format.length(r.durationMs))
        if (r.recovered) add(stringResource(R.string.sub_recovered))
    }.joinToString(" · ")
    return when (r.upload) {
        UploadState.UPLOADING -> LineText(
            stringResource(R.string.st_uploading, (r.progress * 100).toInt()), p.t1, false, base, r.progress,
        )
        UploadState.UPLOADED -> LineText(stringResource(R.string.st_uploaded), p.t3, true, base, null)
        UploadState.WAITING -> {
            val behind = all.any { it.upload == UploadState.UPLOADING && it.id != r.id }
            val state = stringResource(if (wifiOnly) R.string.st_waiting_wifi else R.string.st_waiting)
            val sub = if (behind && detailed) "$base · ${stringResource(R.string.sub_after_previous)}" else base
            LineText(state, p.t3, true, sub, null)
        }
        UploadState.FAILED -> {
            val why = reason(r.error)
            val auto = r.error?.retriesByItself ?: true
            if (detailed) {
                val sub = stringResource(if (auto) R.string.sub_failed_auto else R.string.sub_failed, why)
                LineText(stringResource(R.string.st_retry_now), p.rt, false, "$base · $sub", null)
            } else {
                LineText(stringResource(R.string.st_failed_retry), p.rt, false, "$base · $why", null)
            }
        }
    }
}
