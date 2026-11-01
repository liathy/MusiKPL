package io.github.liathy.musikpl.ui

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.sp
import io.github.liathy.musikpl.R

/** The desktop app's colors (src/theme.rs). */
object Palette {
    val background = Color.Black
    val panel = Color(0xFF101010)
    val surface = Color(0xFF222222)
    val hover = Color(0xFF333333)
    val text = Color.White
    val secondary = Color.White.copy(alpha = 0.6f)
    val muted = Color.White.copy(alpha = 0.4f)
    val icon = Color(0xFFE3E3E3)
    val danger = Color(0xFFE5484D)
    val success = Color(0xFF4CC26C)
    val like = Color(0xFFFF4D6D)
}

val Display = FontFamily(Font(R.font.baloo_thambi2))
val Body = FontFamily(Font(R.font.baloo_thambi))

@Composable
fun MusiKPLTheme(content: @Composable () -> Unit) {
    val colors = darkColorScheme(
        primary = Color.White,
        onPrimary = Color.Black,
        secondary = Palette.secondary,
        background = Palette.background,
        onBackground = Palette.text,
        surface = Palette.panel,
        onSurface = Palette.text,
        surfaceVariant = Palette.surface,
        onSurfaceVariant = Palette.secondary,
        surfaceContainer = Palette.panel,
        surfaceContainerHigh = Palette.surface,
        surfaceContainerHighest = Palette.surface,
        surfaceContainerLow = Palette.panel,
        error = Palette.danger,
    )
    val base = Typography()
    val typography = Typography(
        headlineMedium = TextStyle(fontFamily = Display, fontSize = 28.sp, color = Palette.text),
        titleLarge = TextStyle(fontFamily = Display, fontSize = 22.sp, color = Palette.text),
        titleMedium = TextStyle(fontFamily = Display, fontSize = 17.sp, color = Palette.text),
        bodyLarge = base.bodyLarge.copy(fontFamily = Body, fontSize = 16.sp),
        bodyMedium = base.bodyMedium.copy(fontFamily = Body, fontSize = 14.sp),
        bodySmall = base.bodySmall.copy(fontFamily = Body, fontSize = 12.sp),
        labelLarge = base.labelLarge.copy(fontFamily = Body, fontSize = 15.sp),
        labelMedium = base.labelMedium.copy(fontFamily = Body),
    )
    MaterialTheme(colorScheme = colors, typography = typography, content = content)
}
