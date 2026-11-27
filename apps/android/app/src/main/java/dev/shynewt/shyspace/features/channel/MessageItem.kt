package dev.shynewt.shyspace.features.channel

import androidx.compose.foundation.layout.size
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.material3.minimumInteractiveComponentSize
import androidx.compose.ui.draw.clip
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.customActions
import androidx.compose.ui.semantics.selected
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import dev.shynewt.shyspace.R
import dev.shynewt.shyspace.ui.Avatar
import dev.shynewt.shyspace.ui.Badge
import dev.shynewt.shyspace.ui.Clock
import dev.shynewt.shyspace.ui.displayName
import dev.shynewt.shyspace.ui.theme.LocalShyColors
import java.time.LocalDate
import uniffi.shy_core.Message
import uniffi.shy_core.Reaction

internal val AVATAR_SIZE = 36.dp
internal val GUTTER = 16.dp

@Composable
internal fun dayLabel(date: LocalDate, today: LocalDate): String {
    val locale = LocalConfiguration.current.locales[0]
    return when (val name = dayName(date, today, locale)) {
        DayName.Today -> stringResource(R.string.today)
        DayName.Yesterday -> stringResource(R.string.yesterday)
        is DayName.Date -> name.text
    }
}

@Composable
internal fun MessageItem(
    message: Message,
    text: String,
    isContinuation: Boolean,
    clock: Clock,
    onLongPress: (Message) -> Unit,
    onReact: (message: String, emoji: String, active: Boolean) -> Unit,
    /** Opens the message's thread from its "N replies" line; null inside the thread. */
    onOpenThread: ((Message) -> Unit)?,
) {
    val colors = LocalShyColors.current.palette
    val haptics = LocalHapticFeedback.current
    val time = clock.time(message.createdMs)
    val author = displayName(message.authorName, message.author)
    val actionsLabel = stringResource(R.string.message_actions)
    Row(
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        modifier = Modifier
            .fillMaxWidth()
            .pointerInput(message.id) {
                detectTapGestures(onLongPress = {
                    haptics.performHapticFeedback(HapticFeedbackType.LongPress)
                    onLongPress(message)
                })
            }
            .padding(start = GUTTER, end = GUTTER, top = if (isContinuation) 2.dp else 10.dp, bottom = 2.dp)
            .semantics(mergeDescendants = true) {
                customActions = listOf(CustomAccessibilityAction(actionsLabel) { onLongPress(message); true })
            },
    ) {
        if (isContinuation) {
            // Shown without a name, but a screen reader still says whose it is.
            Spacer(Modifier.width(AVATAR_SIZE).semantics { contentDescription = "$author, $time" })
        } else {
            Avatar(
                id = message.author,
                name = author,
                isBot = message.authorIsBot,
                size = AVATAR_SIZE,
                modifier = Modifier.padding(top = 2.dp),
            )
        }
        Column(modifier = Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            if (!isContinuation) {
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
                    Text(author, style = MaterialTheme.typography.titleSmall, fontWeight = FontWeight.SemiBold)
                    if (message.authorIsBot) Badge(stringResource(R.string.agent))
                    Text(time, style = MaterialTheme.typography.labelSmall, color = colors.textFaint)
                }
            }
            val edited = stringResource(R.string.edited)
            val faint = MaterialTheme.typography.labelSmall.toSpanStyle().copy(color = colors.textFaint)
            Text(
                buildAnnotatedString {
                    append(text)
                    if (message.isEdited) withStyle(faint) { append(" $edited") }
                },
                style = MaterialTheme.typography.bodyLarge,
                color = colors.text,
            )
            if (message.reactions.isNotEmpty()) {
                Reactions(message.reactions, onToggle = { reaction -> onReact(message.id, reaction.emoji, !reaction.mine) })
            }
            // Inside the thread its divider says it, so the line shows only where it opens the thread.
            if (message.replyCount > 0u && onOpenThread != null) {
                val replies = message.replyCount.toInt()
                Text(
                    pluralStringResource(R.plurals.replies, replies, replies),
                    style = MaterialTheme.typography.labelLarge,
                    color = LocalShyColors.current.accent.accentText,
                    modifier = Modifier
                        .padding(top = 2.dp)
                        .clip(MaterialTheme.shapes.small)
                        .clickable(role = Role.Button) { onOpenThread(message) }
                        .minimumInteractiveComponentSize(),
                )
            }
        }
    }
}

@Composable
private fun Reactions(reactions: List<Reaction>, onToggle: (Reaction) -> Unit) {
    val colors = LocalShyColors.current
    FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp), modifier = Modifier.padding(top = 4.dp)) {
        reactions.forEach { reaction ->
            val description = pluralStringResource(R.plurals.reaction, reaction.count.toInt(), reaction.count.toInt(), reaction.emoji)
            Text(
                "${reaction.emoji} ${reaction.count}",
                style = MaterialTheme.typography.labelLarge,
                color = if (reaction.mine) colors.accent.accentText else colors.palette.textMuted,
                modifier = Modifier
                    .minimumInteractiveComponentSize()
                    .clip(CircleShape)
                    .clickable(role = Role.Button) { onToggle(reaction) }
                    .semantics {
                        contentDescription = description
                        selected = reaction.mine
                    }
                    .background(if (reaction.mine) colors.accent.accentSoft else colors.palette.surface2, CircleShape)
                    .border(1.dp, if (reaction.mine) colors.accent.accent else colors.palette.border, CircleShape)
                    .padding(horizontal = 10.dp, vertical = 4.dp),
            )
        }
    }
}

/** Above the message box on a linked phone the workspace hasn't added yet: a still line, no error. */
@Composable
internal fun ReadingOnlyLine() {
    Text(
        stringResource(R.string.reading_only),
        style = MaterialTheme.typography.bodySmall,
        color = LocalShyColors.current.palette.textMuted,
        modifier = Modifier.padding(horizontal = GUTTER, vertical = 4.dp),
    )
}
