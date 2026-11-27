package dev.shynewt.shyspace.features.activity

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import java.util.Locale
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.draw.clip
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.shynewt.shyspace.R
import dev.shynewt.shyspace.features.home.channelTitle
import dev.shynewt.shyspace.ui.Avatar
import dev.shynewt.shyspace.ui.BadgeTone
import dev.shynewt.shyspace.ui.Clock
import dev.shynewt.shyspace.ui.CountBadge
import dev.shynewt.shyspace.ui.SectionLabel
import dev.shynewt.shyspace.ui.displayName
import dev.shynewt.shyspace.ui.rememberClock
import dev.shynewt.shyspace.ui.theme.LocalShyColors
import java.time.Instant
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import uniffi.shy_core.Channel
import uniffi.shy_core.InboxItem
import uniffi.shy_core.InboxReason
import uniffi.shy_core.Member

/**
 * Mentions of you and new replies in threads you follow, then unread channels: the desktop's
 * inbox. [channels] and [members] name the channels as Home does.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ActivityScreen(
    state: ActivityState,
    channels: List<Channel>,
    members: List<Member>,
    onOpenItem: (item: InboxItem, channelTitle: String) -> Unit,
    onOpenChannel: (channel: Channel, title: String) -> Unit,
    onMarkRead: (channelId: String) -> Unit,
    bottomBar: @Composable () -> Unit,
) {
    val background = LocalShyColors.current.palette.bg
    val scrollBehavior = TopAppBarDefaults.pinnedScrollBehavior()
    Scaffold(
        containerColor = background,
        topBar = {
            TopAppBar(
                title = {
                    Text(
                        stringResource(R.string.activity),
                        style = MaterialTheme.typography.titleMedium,
                        fontWeight = FontWeight.SemiBold,
                        modifier = Modifier.semantics { heading() },
                    )
                },
                colors = TopAppBarDefaults.topAppBarColors(containerColor = background, scrolledContainerColor = background),
                scrollBehavior = scrollBehavior,
            )
        },
        bottomBar = bottomBar,
        modifier = Modifier.nestedScroll(scrollBehavior.nestedScrollConnection),
    ) { padding ->
        val isEmpty = state.attention.isEmpty() && state.unreadChannels.isEmpty()
        Column(modifier = Modifier.fillMaxSize().padding(padding)) {
            state.error?.let { error ->
                Text(
                    error,
                    color = LocalShyColors.current.palette.dangerText,
                    style = MaterialTheme.typography.bodyMedium,
                    modifier = Modifier.padding(horizontal = 20.dp, vertical = 8.dp),
                )
            }
            Box(modifier = Modifier.weight(1f).fillMaxWidth()) {
                when {
                    state.isLoading -> CircularProgressIndicator(modifier = Modifier.align(Alignment.Center))
                    isEmpty && state.error != null -> Unit
                    isEmpty -> CaughtUp()
                    else -> ActivityList(state, channels, members, onOpenItem, onOpenChannel, onMarkRead)
                }
            }
        }
    }
}

@Composable
private fun ActivityList(
    state: ActivityState,
    channels: List<Channel>,
    members: List<Member>,
    onOpenItem: (InboxItem, String) -> Unit,
    onOpenChannel: (Channel, String) -> Unit,
    onMarkRead: (String) -> Unit,
) {
    val clock = rememberClock()
    LazyColumn(modifier = Modifier.fillMaxSize().padding(horizontal = 8.dp)) {
        if (state.attention.isNotEmpty()) {
            item { SectionLabel(stringResource(R.string.mentions_and_replies), modifier = Modifier.padding(horizontal = 12.dp)) }
            items(state.attention, key = { "item:" + it.message.id }) { item ->
                val channel = channels.find { it.id == item.message.channel }
                val title = channel?.let { channelTitle(it, members) } ?: "#${item.channelName}"
                AttentionRow(item, title, state.previews[item.message.id] ?: item.message.text, clock) { onOpenItem(item, title) }
            }
        }
        if (state.unreadChannels.isNotEmpty()) {
            item { SectionLabel(stringResource(R.string.unread_channels), modifier = Modifier.padding(horizontal = 12.dp)) }
            items(state.unreadChannels, key = { "channel:" + it.id }) { channel ->
                val title = channelTitle(channel, members)
                UnreadChannelRow(channel, title, onOpen = { onOpenChannel(channel, title) }, onMarkRead = { onMarkRead(channel.id) })
            }
        }
    }
}

@Composable
private fun AttentionRow(item: InboxItem, channelTitle: String, preview: String, clock: Clock, onOpen: () -> Unit) {
    val colors = LocalShyColors.current.palette
    val message = item.message
    val name = displayName(message.authorName, message.author)
    val what = stringResource(
        if (item.reason == InboxReason.MENTION) R.string.mentioned_you_in else R.string.replied_in_thread_in,
        name,
        channelTitle,
    )
    Row(
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        modifier = Modifier
            .fillMaxWidth()
            .clip(MaterialTheme.shapes.small)
            .clickable(role = Role.Button, onClick = onOpen)
            .padding(horizontal = 12.dp, vertical = 10.dp),
    ) {
        Avatar(id = message.author, name = name, isBot = message.authorIsBot, size = 32.dp)
        Column(modifier = Modifier.weight(1f)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(
                    buildAnnotatedString {
                        append(what)
                        val nameAt = what.indexOf(name)
                        if (nameAt >= 0) addStyle(SpanStyle(fontWeight = FontWeight.SemiBold, color = colors.text), nameAt, nameAt + name.length)
                    },
                    style = MaterialTheme.typography.labelLarge,
                    color = colors.textMuted,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f),
                )
                val locale = LocalConfiguration.current.locales[0]
                Text(whenLabel(message.createdMs, clock, locale), style = MaterialTheme.typography.labelMedium, color = colors.textFaint)
            }
            Text(
                preview,
                style = MaterialTheme.typography.bodyMedium,
                color = colors.text,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.padding(top = 2.dp),
            )
        }
    }
}

@Composable
private fun UnreadChannelRow(channel: Channel, title: String, onOpen: () -> Unit, onMarkRead: () -> Unit) {
    val colors = LocalShyColors.current.palette
    val mentions = channel.unreadMentions.toInt()
    val unread = channel.unread.toInt()
    val markReadLabel = stringResource(R.string.mark_read_label, title)
    Row(
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp),
    ) {
        Row(
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            modifier = Modifier
                .weight(1f)
                .heightIn(min = 48.dp)
                .clip(MaterialTheme.shapes.small)
                .clickable(role = Role.Button, onClick = onOpen)
                .padding(horizontal = 12.dp),
        ) {
            Text(
                title,
                style = MaterialTheme.typography.bodyLarge,
                fontWeight = FontWeight.SemiBold,
                color = colors.text,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f, fill = false),
            )
            if (mentions > 0) {
                CountBadge("@$mentions", BadgeTone.DANGER, pluralStringResource(R.plurals.unread_mentions, mentions, mentions))
            }
            CountBadge("$unread", BadgeTone.ACCENT, pluralStringResource(R.plurals.unread_messages, unread, unread))
        }
        TextButton(onClick = onMarkRead, modifier = Modifier.semantics { contentDescription = markReadLabel }) {
            Text(stringResource(R.string.mark_read))
        }
    }
}

@Composable
private fun CaughtUp() {
    val colors = LocalShyColors.current.palette
    Column(
        verticalArrangement = Arrangement.spacedBy(8.dp, Alignment.CenterVertically),
        horizontalAlignment = Alignment.CenterHorizontally,
        modifier = Modifier.fillMaxSize().padding(32.dp),
    ) {
        Icon(painterResource(R.drawable.ic_inbox), contentDescription = null, tint = colors.textDecorative, modifier = Modifier.size(32.dp))
        Text(stringResource(R.string.caught_up), style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold, color = colors.text)
        Text(
            stringResource(R.string.caught_up_detail),
            style = MaterialTheme.typography.bodyMedium,
            color = colors.textMuted,
            textAlign = TextAlign.Center,
        )
    }
}

/** The time for today, the date before that, in the app's language. */
private fun whenLabel(ms: Long, clock: Clock, locale: Locale): String {
    val date = Instant.ofEpochMilli(ms).atZone(clock.zone).toLocalDate()
    return if (date == clock.today) clock.time(ms) else date.format(DateTimeFormatter.ofLocalizedDate(FormatStyle.MEDIUM).withLocale(locale))
}
