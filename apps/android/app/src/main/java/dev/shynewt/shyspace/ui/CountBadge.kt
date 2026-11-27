package dev.shynewt.shyspace.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import dev.shynewt.shyspace.ui.theme.LocalShyColors

enum class BadgeTone {
    /** New things for you, like unread messages. */
    ACCENT,

    /** Someone needs you: mentions. */
    DANGER,

    /** Only waiting, like a draft. */
    QUIET,
}

/** A count pill ("3", "@2"), read out as [description]. */
@Composable
fun CountBadge(text: String, tone: BadgeTone, description: String, modifier: Modifier = Modifier) {
    val colors = LocalShyColors.current
    val (fill, ink) = when (tone) {
        BadgeTone.ACCENT -> colors.accent.accent to colors.accent.onAccent
        BadgeTone.DANGER -> colors.palette.danger to androidx.compose.ui.graphics.Color.White
        BadgeTone.QUIET -> colors.palette.surface3 to colors.palette.textMuted
    }
    Text(
        text,
        color = ink,
        style = MaterialTheme.typography.labelMedium,
        fontWeight = FontWeight.SemiBold,
        textAlign = TextAlign.Center,
        modifier = modifier
            .semantics { contentDescription = description }
            .background(fill, CircleShape)
            .widthIn(min = 22.dp)
            .padding(horizontal = 7.dp, vertical = 2.dp),
    )
}
