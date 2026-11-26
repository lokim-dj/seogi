package io.github.lokimdj.seogi.ui

import android.provider.Settings
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.TextUnit
import androidx.compose.ui.unit.em
import androidx.compose.ui.unit.sp
import io.github.lokimdj.seogi.R

/**
 * The colours of the chosen design (galaxy-screens, A): a warm ink black that
 * is the first impression, the same layout on paper for a light system, red
 * only for recording and for what needs a look.
 */
@Immutable
data class Palette(
    val bg: Color, val s0: Color, val s1: Color, val s2: Color, val line: Color,
    val t1: Color, val t2: Color, val t3: Color, val red: Color, val rt: Color,
)

val Dark = Palette(
    bg = Color(0xFF161311), s0 = Color(0xFF0E0C0A), s1 = Color(0xFF1E1B18), s2 = Color(0xFF2A2622),
    line = Color(0xFF36322F), t1 = Color(0xFFEDEBE7), t2 = Color(0xFFD1CDC8), t3 = Color(0xFFA8A49F),
    red = Color(0xFFE9483A), rt = Color(0xFFF87868),
)

val Light = Palette(
    bg = Color(0xFFF2F0ED), s0 = Color(0xFFE5E2DF), s1 = Color(0xFFE8E6E3), s2 = Color(0xFFD7D4D0),
    line = Color(0xFFD0CDCA), t1 = Color(0xFF1E1A16), t2 = Color(0xFF3C3733), t3 = Color(0xFF5F5A55),
    red = Color(0xFFD02D27), rt = Color(0xFFBE241F),
)

val LocalPalette = staticCompositionLocalOf { Dark }
val LocalReduceMotion = staticCompositionLocalOf { false }

val Gmarket = FontFamily(
    Font(R.font.gmarket_medium, FontWeight.Medium),
    Font(R.font.gmarket_bold, FontWeight.Bold),
)

@Composable
fun SeogiTheme(content: @Composable () -> Unit) {
    val reduce = Settings.Global.getFloat(
        LocalContext.current.contentResolver, Settings.Global.ANIMATOR_DURATION_SCALE, 1f,
    ) == 0f
    CompositionLocalProvider(
        LocalPalette provides if (isSystemInDarkTheme()) Dark else Light,
        LocalReduceMotion provides reduce,
        content = content,
    )
}

val pal: Palette @Composable get() = LocalPalette.current

/** Text in Gmarket Sans: Bold for reading, Medium for the quiet lines. */
@Composable
fun T(
    text: String,
    modifier: Modifier = Modifier,
    size: TextUnit = 15.sp,
    color: Color = pal.t1,
    medium: Boolean = false,
    align: TextAlign = TextAlign.Start,
    maxLines: Int = Int.MAX_VALUE,
    letterSpacing: TextUnit = (-0.01).em,
    lineHeight: Float = 1.45f,
    tabular: Boolean = false,
) {
    BasicText(
        text = text,
        modifier = modifier,
        maxLines = maxLines,
        overflow = TextOverflow.Ellipsis,
        style = TextStyle(
            fontFamily = Gmarket,
            fontWeight = if (medium) FontWeight.Medium else FontWeight.Bold,
            fontSize = size,
            color = color,
            textAlign = align,
            letterSpacing = letterSpacing,
            lineHeight = size * lineHeight,
            fontFeatureSettings = if (tabular) "tnum" else null,
        ),
    )
}
