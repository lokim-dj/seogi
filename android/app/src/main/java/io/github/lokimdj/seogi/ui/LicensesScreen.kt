package io.github.lokimdj.seogi.ui

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.lokimdj.seogi.R

/** One part of the app made by others, and the licence it comes under. */
data class OpenSourcePart(val name: String, val licenseRes: Int)

/** What the app ships that others made; the full licence texts are in res/raw. */
val OpenSourceParts = listOf(
    OpenSourcePart("AndroidX (Core, Activity, Lifecycle, Compose, WorkManager, Credentials)", R.raw.license_apache_2_0),
    OpenSourcePart("Kotlin, kotlinx.coroutines, kotlinx.serialization", R.raw.license_apache_2_0),
    OpenSourcePart("OkHttp, Okio", R.raw.license_apache_2_0),
    OpenSourcePart("Google Identity Services (play-services-auth), Google ID", R.raw.license_android_sdk),
    OpenSourcePart("Pretendard", R.raw.license_ofl_pretendard),
    OpenSourcePart("transcribe.cpp, ggml, Silero VAD, miniz (말로 입력의 음성 인식)", R.raw.license_transcribe_cpp),
    OpenSourcePart("Qwen3-ASR 0.6B 모델 (처음 쓸 때 받음)", R.raw.license_apache_2_0),
)

/** Settings › 오픈소스 라이선스: the parts, then each licence in full, once. */
@Composable
fun LicensesScreen(onBack: () -> Unit) {
    val context = LocalContext.current
    val texts = remember {
        OpenSourceParts.map { it.licenseRes }.distinct().associateWith { res ->
            context.resources.openRawResource(res).bufferedReader().use { it.readText() }
        }
    }
    Column(Modifier.fillMaxSize().padding(horizontal = Gutter)) {
        Spacer(Modifier.height(10.dp))
        BackBar(stringResource(R.string.set_licenses), onBack)
        LazyColumn(Modifier.weight(1f)) {
            items(OpenSourceParts) { part ->
                Hairline()
                T(part.name, size = 14.sp, modifier = Modifier.padding(vertical = 10.dp))
            }
            items(texts.entries.toList()) { (_, text) ->
                Hairline()
                T(text, size = 11.sp, color = pal.t2, quiet = true, modifier = Modifier.padding(vertical = 14.dp))
            }
        }
    }
}
