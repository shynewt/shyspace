package dev.shynewt.shyspace.ui.theme

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.ColorScheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Shapes
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp

/** The shown theme's colours, including those Material has no slot for (faint and decorative text). */
data class ShyThemeColors(val palette: ShyColors, val accent: ShyAccentColors)

val LocalShyColors = staticCompositionLocalOf<ShyThemeColors> {
    error("ShyTheme isn't applied above this composable")
}

/** The desktop's shapes: controls 8, cards 12, dialogs and sheets 16. */
private val shyShapes = Shapes(
    small = RoundedCornerShape(8.dp),
    medium = RoundedCornerShape(12.dp),
    large = RoundedCornerShape(16.dp),
)

@Composable
fun ShyTheme(
    palette: ShyPalette = ShyPalette.SHY,
    accent: ShyAccent = ShyAccent.VIOLET,
    isDark: Boolean = isSystemInDarkTheme(),
    content: @Composable () -> Unit,
) {
    val colors = ShyThemeColors(
        palette = if (isDark) palette.dark else palette.light,
        accent = if (isDark) accent.dark else accent.light,
    )
    CompositionLocalProvider(LocalShyColors provides colors) {
        MaterialTheme(
            colorScheme = materialColors(colors, isDark),
            shapes = shyShapes,
            content = content,
        )
    }
}

private fun materialColors(colors: ShyThemeColors, isDark: Boolean): ColorScheme {
    val (palette, accent) = colors
    val base = if (isDark) darkColorScheme() else lightColorScheme()
    return base.copy(
        primary = accent.accent,
        onPrimary = accent.onAccent,
        primaryContainer = accent.accentSoft,
        onPrimaryContainer = accent.accentText,
        secondary = accent.accentStrong,
        onSecondary = accent.onAccent,
        secondaryContainer = accent.accentSoft,
        onSecondaryContainer = accent.accentText,
        background = palette.bg,
        onBackground = palette.text,
        surface = palette.surface,
        onSurface = palette.text,
        surfaceVariant = palette.surface2,
        onSurfaceVariant = palette.textMuted,
        surfaceContainerLowest = palette.surface,
        surfaceContainerLow = palette.bg,
        surfaceContainer = palette.surface2,
        surfaceContainerHigh = palette.surface2,
        surfaceContainerHighest = palette.surface3,
        outline = palette.border,
        outlineVariant = palette.border,
        error = palette.danger,
        // The danger fill is chosen so white text passes on it in every theme.
        onError = Color.White,
        errorContainer = palette.surface2,
        onErrorContainer = palette.dangerText,
    )
}
