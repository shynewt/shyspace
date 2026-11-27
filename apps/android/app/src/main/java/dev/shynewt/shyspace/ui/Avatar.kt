package dev.shynewt.shyspace.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import dev.shynewt.shyspace.R
import kotlin.math.abs

private val HUES = intArrayOf(262, 200, 152, 32, 340, 12, 288, 176)

/** A stable hue per member from their id: the desktop's hash, so a person has one colour everywhere. */
fun avatarHue(id: String): Int {
    var hash = 0
    for (char in id) hash = hash * 31 + char.code
    return HUES[abs(hash) % HUES.size]
}

/** A member's colour, as their avatar has it, for marks that stand for them. */
fun memberColor(id: String): Color = Color.hsl(avatarHue(id).toFloat(), 0.70f, 0.52f)

fun initials(name: String): String {
    val words = name.trim().split(Regex("\\s+")).filter { it.isNotEmpty() }
    if (words.isEmpty()) return "?"
    return (words[0].take(1) + (words.getOrNull(1)?.take(1) ?: "")).uppercase()
}

/**
 * A member's tile: their initials (an agent's robot) on their colour, in the desktop's rounded
 * square. Decorative: say the name beside it.
 */
@Composable
fun Avatar(id: String, name: String, modifier: Modifier = Modifier, size: Dp = 36.dp, isBot: Boolean = false) {
    val hue = avatarHue(id).toFloat()
    // The desktop's 140° gradient: from the top left, a little past the bottom right.
    val gradient = Brush.linearGradient(
        colors = listOf(Color.hsl(hue, 0.70f, 0.62f), Color.hsl((hue + 24) % 360, 0.62f, 0.48f)),
        start = Offset.Zero,
        end = Offset.Infinite,
    )
    Box(
        contentAlignment = Alignment.Center,
        modifier = modifier
            .size(size)
            .background(gradient, RoundedCornerShape(percent = 30))
            .clearAndSetSemantics {},
    ) {
        if (isBot) {
            Icon(painterResource(R.drawable.ic_bot), contentDescription = null, tint = Color.White, modifier = Modifier.size(size * 0.55f))
        } else {
            Text(
                initials(name),
                color = Color.White,
                fontWeight = FontWeight.SemiBold,
                fontSize = with(LocalDensity.current) { (size * 0.38f).toSp() },
            )
        }
    }
}
