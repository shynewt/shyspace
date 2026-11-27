package dev.shynewt.shyspace.features.home

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.shynewt.shyspace.R
import dev.shynewt.shyspace.ui.Avatar
import dev.shynewt.shyspace.ui.BadgeTone
import dev.shynewt.shyspace.ui.CountBadge
import dev.shynewt.shyspace.ui.SectionLabel
import dev.shynewt.shyspace.ui.displayName
import dev.shynewt.shyspace.ui.theme.LocalShyColors
import uniffi.shy_core.Channel
import uniffi.shy_core.Member
import uniffi.shy_ffi.BucketSettings
import uniffi.shy_core.ChannelKind
import uniffi.shy_core.NotifyLevel

/** The workspace's channels, or what to do when there's none. */
@Composable
fun HomeScreen(
    state: HomeState,
    onOpenChannel: (channel: Channel, title: String) -> Unit,
    onRetry: () -> Unit,
    bottomBar: @Composable () -> Unit,
    noWorkspaceContent: @Composable (HomeState.NoWorkspace) -> Unit,
) {
    when (state) {
        HomeState.Loading -> Centered { CircularProgressIndicator() }
        is HomeState.NoWorkspace -> noWorkspaceContent(state)
        is HomeState.Failed -> OpenFailed(state.reason, onRetry)
        is HomeState.Ready -> ChannelList(state, onOpenChannel, bottomBar)
    }
}

/** The name a channel shows with: `#name`, Notes, or for a direct message, the others' names. */
@Composable
fun channelTitle(channel: Channel, members: List<Member>): String = when (channel.kind) {
    ChannelKind.NOTES -> stringResource(R.string.notes)
    // Everyone else may have left; the row still needs a name.
    ChannelKind.DM -> channel.dmMembers.joinToString { id -> displayName(members.find { it.id == id }?.name.orEmpty(), id) }
        .ifEmpty { stringResource(R.string.direct_message) }
    else -> "#${channel.name}"
}

@Composable
private fun ChannelList(state: HomeState.Ready, onOpenChannel: (Channel, String) -> Unit, bottomBar: @Composable () -> Unit) {
    val background = LocalShyColors.current.palette.bg
    val scrollBehavior = TopAppBarDefaults.pinnedScrollBehavior()
    val joined = state.channels.filter { it.isMember }
    val channels = joined.filter { it.kind == ChannelKind.PUBLIC || it.kind == ChannelKind.PRIVATE }
    // As on the desktop, your notes lead the direct messages.
    val direct = joined.filter { it.kind == ChannelKind.NOTES } + joined.filter { it.kind == ChannelKind.DM }
    Scaffold(
        containerColor = background,
        topBar = {
            TopAppBar(
                title = {
                    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                        Avatar(id = state.workspaceId, name = state.workspaceName, size = 32.dp)
                        Text(
                            state.workspaceName,
                            style = MaterialTheme.typography.titleMedium,
                            fontWeight = FontWeight.SemiBold,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                            modifier = Modifier.semantics { heading() },
                        )
                    }
                },
                colors = TopAppBarDefaults.topAppBarColors(containerColor = background, scrolledContainerColor = background),
                scrollBehavior = scrollBehavior,
            )
        },
        bottomBar = bottomBar,
        modifier = Modifier.nestedScroll(scrollBehavior.nestedScrollConnection),
    ) { padding ->
        LazyColumn(modifier = Modifier.fillMaxSize().padding(padding).padding(horizontal = 8.dp)) {
            if (state.openFailures.isNotEmpty()) {
                item {
                    Text(
                        stringResource(R.string.open_failed) + ": " + state.openFailures.joinToString { it.reason },
                        color = LocalShyColors.current.palette.dangerText,
                        style = MaterialTheme.typography.bodyMedium,
                        modifier = Modifier.padding(12.dp),
                    )
                }
            }
            item { SectionLabel(stringResource(R.string.channels), modifier = Modifier.padding(horizontal = 12.dp)) }
            items(channels, key = { it.id }) { channel -> ChannelRow(channel, state.members, onOpen = onOpenChannel) }
            if (direct.isNotEmpty()) {
                item { SectionLabel(stringResource(R.string.direct_messages), modifier = Modifier.padding(horizontal = 12.dp)) }
            }
            items(direct, key = { it.id }) { channel -> ChannelRow(channel, state.members, onOpen = onOpenChannel) }
        }
    }
}

@Composable
private fun ChannelRow(channel: Channel, members: List<Member>, onOpen: (Channel, String) -> Unit) {
    val colors = LocalShyColors.current.palette
    val title = channelTitle(channel, members)
    val isMuted = channel.notify == NotifyLevel.NOTHING
    val hasUnread = channel.unread > 0u
    val mentions = channel.unreadMentions.toInt()
    val (icon, name) = when (channel.kind) {
        ChannelKind.PRIVATE -> R.drawable.ic_lock to channel.name
        ChannelKind.NOTES, ChannelKind.DM -> R.drawable.ic_notes to title
        ChannelKind.PUBLIC -> R.drawable.ic_tag to channel.name
    }
    // A direct message shows its first other person instead of an icon.
    val dmPerson = channel.dmMembers.firstOrNull()?.takeIf { channel.kind == ChannelKind.DM }?.let { id ->
        members.find { it.id == id } to id
    }
    val ink = when {
        hasUnread && !isMuted -> colors.text
        isMuted -> colors.textFaint
        else -> colors.textMuted
    }
    // The robot avatar is hidden from screen readers, so a DM with an agent says so in words.
    val isWithAgent = dmPerson?.first?.isBot == true
    val state = listOfNotNull(
        stringResource(R.string.agent).takeIf { isWithAgent },
        when {
            isMuted -> stringResource(R.string.muted)
            hasUnread -> stringResource(R.string.unread)
            else -> null
        },
    ).joinToString(", ").ifEmpty { null }
    Row(
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        modifier = Modifier
            .fillMaxWidth()
            .clip(MaterialTheme.shapes.small)
            .clickable { onOpen(channel, title) }
            .heightIn(min = 48.dp)
            .padding(horizontal = 12.dp)
            .semantics(mergeDescendants = true) { state?.let { stateDescription = it } },
    ) {
        if (dmPerson != null) {
            val (member, id) = dmPerson
            Avatar(id = id, name = displayName(member?.name.orEmpty(), id), isBot = member?.isBot == true, size = 20.dp)
        } else {
            val iconLabel = if (channel.kind == ChannelKind.PRIVATE) stringResource(R.string.private_channel) else null
            Icon(painterResource(icon), contentDescription = iconLabel, tint = if (hasUnread && !isMuted) colors.text else colors.textDecorative, modifier = Modifier.size(20.dp))
        }
        Text(
            name,
            style = MaterialTheme.typography.bodyLarge,
            fontWeight = if (hasUnread && !isMuted) FontWeight.SemiBold else FontWeight.Normal,
            color = ink,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.weight(1f),
        )
        if (mentions > 0) {
            CountBadge(
                text = "@$mentions",
                tone = BadgeTone.DANGER,
                description = pluralStringResource(R.plurals.unread_mentions, mentions, mentions),
            )
        }
    }
}

@Composable
private fun OpenFailed(reason: String, onRetry: () -> Unit) {
    Column(
        verticalArrangement = Arrangement.spacedBy(16.dp, Alignment.CenterVertically),
        horizontalAlignment = Alignment.CenterHorizontally,
        modifier = Modifier.fillMaxSize().padding(24.dp),
    ) {
        Text(stringResource(R.string.open_failed_title), style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
        Text(reason, style = MaterialTheme.typography.bodyMedium, color = LocalShyColors.current.palette.textMuted, textAlign = TextAlign.Center)
        Button(onClick = onRetry) { Text(stringResource(R.string.try_again)) }
    }
}

@Composable
private fun Centered(content: @Composable () -> Unit) {
    Box(modifier = Modifier.fillMaxSize(), contentAlignment = Alignment.Center) { content() }
}

/** A way to create a workspace from the phone, which only development builds have (`workspaceCreator`). */
typealias WorkspaceCreator = @Composable (
    isCreating: Boolean,
    onCreate: (workspaceName: String, memberName: String, bucket: BucketSettings) -> Unit,
) -> Unit
