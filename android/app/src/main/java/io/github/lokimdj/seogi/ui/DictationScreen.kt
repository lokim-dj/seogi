package io.github.lokimdj.seogi.ui

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.em
import androidx.compose.ui.unit.sp
import io.github.lokimdj.seogi.R

/**
 * Dictation's place. The floating key, the accessibility step and the
 * on-phone speech engine come in the next version (see the design).
 */
@Composable
fun DictationScreen(onBack: () -> Unit) {
    Column(Modifier.fillMaxSize().padding(horizontal = Gutter)) {
        Spacer(Modifier.height(10.dp))
        BackBar(stringResource(R.string.dictation), onBack)
        T(
            stringResource(R.string.dict_title), size = 23.sp, letterSpacing = (-0.03).em, lineHeight = 1.25f,
            modifier = Modifier.padding(top = 22.dp),
        )
        T(stringResource(R.string.dict_body), size = 14.5.sp, color = pal.t2, medium = true, modifier = Modifier.padding(top = 12.dp))
        Spacer(Modifier.height(20.dp))
        Hairline()
        T(stringResource(R.string.dict_steps), size = 13.sp, color = pal.t3, medium = true, modifier = Modifier.padding(top = 14.dp))
    }
}
