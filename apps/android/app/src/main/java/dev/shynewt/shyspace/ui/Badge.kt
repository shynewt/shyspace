package dev.shynewt.shyspace.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import dev.shynewt.shyspace.ui.theme.LocalShyColors

/** A short word set apart beside a name ("Agent"), in the accent's soft colours. */
@Composable
fun Badge(text: String, modifier: Modifier = Modifier) {
    val accent = LocalShyColors.current.accent
    Text(
        text,
        style = MaterialTheme.typography.labelSmall,
        fontWeight = FontWeight.SemiBold,
        color = accent.accentText,
        modifier = modifier.background(accent.accentSoft, RoundedCornerShape(6.dp)).padding(horizontal = 6.dp, vertical = 1.dp),
    )
}
