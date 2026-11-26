package dev.shynewt.shyspace.features.home

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.Badge
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.LargeTopAppBar
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import dev.shynewt.shyspace.R
import dev.shynewt.shyspace.ui.theme.LocalShyColors
import uniffi.shy_core.Channel
import uniffi.shy_ffi.BucketSettings
import uniffi.shy_core.ChannelKind

/** The workspace's channels, or what to do when there's none. */
@Composable
fun HomeScreen(
    state: HomeState,
    onOpenChannel: (channel: Channel, title: String) -> Unit,
    noWorkspaceContent: @Composable () -> Unit,
) {
    when (state) {
        HomeState.Loading -> Centered { CircularProgressIndicator() }
        HomeState.NoWorkspace -> noWorkspaceContent()
        is HomeState.Failed -> Centered { Text(state.reason, color = LocalShyColors.current.palette.dangerText) }
        is HomeState.Ready -> ChannelList(state, onOpenChannel)
    }
}

/** The name a channel shows with: `#name`, or Notes. */
@Composable
fun channelTitle(channel: Channel): String = when (channel.kind) {
    ChannelKind.NOTES -> stringResource(R.string.notes)
    else -> "#${channel.name}"
}

@Composable
private fun ChannelList(state: HomeState.Ready, onOpenChannel: (Channel, String) -> Unit) {
    val scrollBehavior = TopAppBarDefaults.exitUntilCollapsedScrollBehavior()
    // DMs need their members' names, which the phone doesn't load yet.
    val shown = state.channels.filter { it.isMember && it.kind != ChannelKind.DM }
    Scaffold(
        topBar = { LargeTopAppBar(title = { Text(state.workspaceName) }, scrollBehavior = scrollBehavior) },
        modifier = Modifier.nestedScroll(scrollBehavior.nestedScrollConnection),
    ) { padding ->
        LazyColumn(modifier = Modifier.fillMaxSize().padding(padding)) {
            if (state.openFailures.isNotEmpty()) {
                item {
                    Text(
                        stringResource(R.string.open_failed) + ": " + state.openFailures.joinToString { it.reason },
                        color = LocalShyColors.current.palette.dangerText,
                        modifier = Modifier.padding(16.dp),
                    )
                }
            }
            items(shown, key = { it.id }) { channel -> ChannelRow(channel, onOpen = onOpenChannel) }
        }
    }
}

@Composable
private fun ChannelRow(channel: Channel, onOpen: (Channel, String) -> Unit) {
    val title = channelTitle(channel)
    val hasUnread = channel.unread > 0u
    val mentions = channel.unreadMentions.toInt()
    val mentionLabel = if (mentions > 0) pluralStringResource(R.plurals.unread_mentions, mentions, mentions) else null
    ListItem(
        trailingContent = mentionLabel?.let {
            {
                Badge(
                    containerColor = MaterialTheme.colorScheme.primary,
                    modifier = Modifier.semantics { contentDescription = it },
                ) { Text(mentions.toString()) }
            }
        },
        colors = ListItemDefaults.colors(containerColor = MaterialTheme.colorScheme.background),
        modifier = Modifier.fillMaxWidth().clickable { onOpen(channel, title) },
    ) {
        Text(
            title,
            fontWeight = if (hasUnread) FontWeight.SemiBold else FontWeight.Normal,
            color = if (hasUnread) MaterialTheme.colorScheme.onSurface else MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

@Composable
private fun Centered(content: @Composable () -> Unit) {
    Box(modifier = Modifier.fillMaxSize(), contentAlignment = Alignment.Center) { content() }
}

@Composable
fun NoWorkspace(extra: @Composable () -> Unit) {
    Column(
        verticalArrangement = Arrangement.spacedBy(16.dp, Alignment.CenterVertically),
        horizontalAlignment = Alignment.CenterHorizontally,
        modifier = Modifier
            .fillMaxSize()
            .imePadding()
            .verticalScroll(rememberScrollState())
            .padding(24.dp),
    ) {
        Text(stringResource(R.string.app_name), style = MaterialTheme.typography.headlineMedium)
        Text(stringResource(R.string.no_workspace), color = MaterialTheme.colorScheme.onSurfaceVariant)
        extra()
    }
}

/** A way to create a workspace from the phone, which only development builds have (`workspaceCreator`). */
typealias WorkspaceCreator = @Composable (onCreate: (workspaceName: String, memberName: String, bucket: BucketSettings) -> Unit) -> Unit
