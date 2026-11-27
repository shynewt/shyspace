package dev.shynewt.shyspace.features.channel

import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.runtime.snapshotFlow
import kotlinx.coroutines.flow.first
import androidx.compose.runtime.LaunchedEffect
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.repeatOnLifecycle
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.input.TextFieldLineLimits
import androidx.compose.foundation.text.input.clearText
import androidx.compose.foundation.text.input.rememberTextFieldState
import androidx.compose.material3.FilledIconButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.IconButtonDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
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
import dev.shynewt.shyspace.ui.rememberClock
import dev.shynewt.shyspace.ui.DayDivider
import dev.shynewt.shyspace.ui.theme.LocalShyColors
import java.time.LocalDate
import kotlinx.coroutines.launch
import uniffi.shy_core.Message
import uniffi.shy_core.Reaction

private val AVATAR_SIZE = 36.dp
private val GUTTER = 16.dp

/** A channel: its messages, newest at the bottom, and a composer that rides on the keyboard. */
@Composable
fun ChannelScreen(
    title: String,
    state: ChannelState,
    onBack: () -> Unit,
    onSend: suspend (String) -> Boolean,
    onSeen: (newestMessage: String) -> Unit,
) {
    val listState = rememberLazyListState()
    val newest = state.messages.firstOrNull()?.id
    // Read only once the newest message is in view, and the screen really in front: not when
    // scrolled up reading older ones, nor left open behind another app.
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    LaunchedEffect(newest, lifecycle) {
        if (newest == null) return@LaunchedEffect
        lifecycle.repeatOnLifecycle(Lifecycle.State.RESUMED) {
            snapshotFlow { listState.firstVisibleItemIndex == 0 }.first { it }
            onSeen(newest)
        }
    }
    val surface = LocalShyColors.current.palette.surface
    Scaffold(
        containerColor = surface,
        topBar = {
            Column {
                TopAppBar(
                    title = { Text(title, style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold, modifier = Modifier.semantics { heading() }) },
                    navigationIcon = {
                        IconButton(onClick = onBack) {
                            Icon(painterResource(R.drawable.ic_arrow_back), contentDescription = stringResource(R.string.back))
                        }
                    },
                    colors = TopAppBarDefaults.topAppBarColors(containerColor = surface, scrolledContainerColor = surface),
                )
                HorizontalDivider(color = LocalShyColors.current.palette.border)
            }
        },
    ) { padding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(top = padding.calculateTopPadding())
                .imePadding(),
        ) {
            Box(modifier = Modifier.weight(1f)) {
                when {
                    state.messages.isEmpty() && !state.isLoading -> EmptyChannel()
                    else -> MessageList(state.messages, listState, textOf = state::textOf)
                }
            }
            state.error?.let { error ->
                Text(
                    error,
                    color = LocalShyColors.current.palette.dangerText,
                    style = MaterialTheme.typography.bodySmall,
                    modifier = Modifier.padding(horizontal = GUTTER, vertical = 4.dp),
                )
            }
            Composer(title = title, onSend = onSend)
        }
    }
}

@Composable
private fun EmptyChannel() {
    Box(modifier = Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
        Text(stringResource(R.string.no_messages), color = LocalShyColors.current.palette.textMuted)
    }
}

@Composable
private fun MessageList(newestFirst: List<Message>, listState: LazyListState, textOf: (Message) -> String) {
    val clock = rememberClock()
    val rows = remember(newestFirst, clock.zone) { channelRows(newestFirst, clock.zone) }
    // Reversed, so the newest sits at the bottom and new ones keep the view there.
    LazyColumn(
        state = listState,
        reverseLayout = true,
        contentPadding = PaddingValues(top = 8.dp, bottom = 12.dp),
        modifier = Modifier.fillMaxSize(),
    ) {
        items(rows, key = { it.key }) { row ->
            when (row) {
                is ChannelRow.Day -> DayDivider(dayLabel(row.date, clock.today), modifier = Modifier.padding(horizontal = GUTTER))
                is ChannelRow.Item -> MessageItem(row.message, textOf(row.message), row.isContinuation, clock)
            }
        }
    }
}

@Composable
private fun dayLabel(date: LocalDate, today: LocalDate): String {
    val locale = LocalConfiguration.current.locales[0]
    return when (val name = dayName(date, today, locale)) {
        DayName.Today -> stringResource(R.string.today)
        DayName.Yesterday -> stringResource(R.string.yesterday)
        is DayName.Date -> name.text
    }
}

@Composable
private fun MessageItem(message: Message, text: String, isContinuation: Boolean, clock: Clock) {
    val colors = LocalShyColors.current.palette
    val time = clock.time(message.createdMs)
    val author = displayName(message.authorName, message.author)
    Row(
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        modifier = Modifier
            .fillMaxWidth()
            .padding(start = GUTTER, end = GUTTER, top = if (isContinuation) 2.dp else 10.dp)
            .semantics(mergeDescendants = true) {},
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
            if (message.reactions.isNotEmpty()) Reactions(message.reactions)
            if (message.replyCount > 0u) {
                val replies = message.replyCount.toInt()
                Text(
                    pluralStringResource(R.plurals.replies, replies, replies),
                    style = MaterialTheme.typography.labelLarge,
                    color = LocalShyColors.current.accent.accentText,
                    modifier = Modifier.padding(top = 2.dp),
                )
            }
        }
    }
}

@Composable
private fun Reactions(reactions: List<Reaction>) {
    val colors = LocalShyColors.current
    FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp), modifier = Modifier.padding(top = 4.dp)) {
        reactions.forEach { reaction ->
            val description = pluralStringResource(R.plurals.reaction, reaction.count.toInt(), reaction.count.toInt(), reaction.emoji)
            Text(
                "${reaction.emoji} ${reaction.count}",
                style = MaterialTheme.typography.labelLarge,
                color = if (reaction.mine) colors.accent.accentText else colors.palette.textMuted,
                modifier = Modifier
                    .semantics { contentDescription = description }
                    .background(if (reaction.mine) colors.accent.accentSoft else colors.palette.surface2, CircleShape)
                    .border(1.dp, if (reaction.mine) colors.accent.accent else colors.palette.border, CircleShape)
                    .padding(horizontal = 10.dp, vertical = 4.dp),
            )
        }
    }
}

@Composable
private fun Composer(title: String, onSend: suspend (String) -> Boolean) {
    val draft = rememberTextFieldState()
    val scope = rememberCoroutineScope()
    val colors = LocalShyColors.current
    val hasText = draft.text.isNotBlank()
    Row(
        verticalAlignment = Alignment.Bottom,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        modifier = Modifier
            .fillMaxWidth()
            .navigationBarsPadding()
            .padding(start = 12.dp, end = 8.dp, top = 6.dp, bottom = 8.dp),
    ) {
        val placeholder = stringResource(R.string.message_placeholder, title)
        BasicTextField(
            state = draft,
            textStyle = MaterialTheme.typography.bodyLarge.copy(color = colors.palette.text),
            cursorBrush = SolidColor(colors.accent.accent),
            lineLimits = TextFieldLineLimits.MultiLine(maxHeightInLines = 6),
            modifier = Modifier
                .weight(1f)
                .heightIn(min = 48.dp)
                .semantics { contentDescription = placeholder },
            decorator = { field ->
                Box(
                    contentAlignment = Alignment.CenterStart,
                    modifier = Modifier
                        .background(colors.palette.surface2, MaterialTheme.shapes.large)
                        .border(1.dp, colors.palette.border, MaterialTheme.shapes.large)
                        .padding(horizontal = 14.dp, vertical = 10.dp),
                ) {
                    if (draft.text.isEmpty()) Text(placeholder, style = MaterialTheme.typography.bodyLarge, color = colors.palette.textFaint)
                    field()
                }
            },
        )
        FilledIconButton(
            onClick = { scope.launch { if (onSend(draft.text.toString())) draft.clearText() } },
            enabled = hasText,
            colors = IconButtonDefaults.filledIconButtonColors(
                containerColor = colors.accent.accent,
                contentColor = colors.accent.onAccent,
                disabledContainerColor = colors.palette.surface2,
                disabledContentColor = colors.palette.textDecorative,
            ),
            modifier = Modifier.padding(bottom = 2.dp),
        ) {
            Icon(painterResource(R.drawable.ic_send), contentDescription = stringResource(R.string.send))
        }
    }
}
