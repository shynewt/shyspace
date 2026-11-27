package dev.shynewt.shyspace.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import dev.shynewt.shyspace.ui.theme.LocalShyColors

/** A day's heading in a dated list ("Today", "Monday, April 27"), between two hairlines. */
@Composable
fun DayDivider(label: String, modifier: Modifier = Modifier) {
    val colors = LocalShyColors.current.palette
    Row(
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        modifier = modifier.fillMaxWidth().padding(vertical = 12.dp),
    ) {
        HorizontalDivider(color = colors.border, modifier = Modifier.weight(1f))
        Text(
            label,
            style = MaterialTheme.typography.labelMedium,
            color = colors.textFaint,
            modifier = Modifier.semantics { heading() },
        )
        HorizontalDivider(color = colors.border, modifier = Modifier.weight(1f))
    }
}
