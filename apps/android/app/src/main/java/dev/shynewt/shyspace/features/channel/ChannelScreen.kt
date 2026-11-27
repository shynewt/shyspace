package dev.shynewt.shyspace.features.channel

import androidx.compose.foundation.layout.size
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.platform.LocalClipboard
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.runtime.snapshotFlow
import kotlinx.coroutines.flow.first
import androidx.compose.runtime.LaunchedEffect
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.repeatOnLifecycle
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
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
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import dev.shynewt.shyspace.R
import dev.shynewt.shyspace.ui.rememberClock
import dev.shynewt.shyspace.ui.DayDivider
import dev.shynewt.shyspace.ui.theme.LocalShyColors
import kotlinx.coroutines.launch
import uniffi.shy_core.Message


/** A channel: its messages, newest at the bottom, and a composer that rides on the keyboard. */
@Composable
fun ChannelScreen(
    title: String,
    state: ChannelState,
    onBack: () -> Unit,
    onSend: suspend (String) -> Boolean,
    onSeen: (newestMessage: String) -> Unit,
    onEdit: suspend (message: String, text: String) -> Boolean,
    onReact: (message: String, emoji: String, active: Boolean) -> Unit,
    onDelete: (message: String) -> Unit,
    onLoadOlder: () -> Unit,
    onOpenThread: (root: Message) -> Unit,
) {
    val scope = rememberCoroutineScope()
    val clipboard = LocalClipboard.current
    val draft = rememberMessageDraft()
    var actionsFor by remember { mutableStateOf<Message?>(null) }
    var deleting by remember { mutableStateOf<Message?>(null) }
    LaunchedEffect(state, draft.editingId) {
        val editing = draft.editingId ?: return@LaunchedEffect
        if (!state.isLoading && state.messages.none { it.id == editing }) draft.editedMessageGone()
    }
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
                    else -> MessageList(
                        state.messages,
                        listState,
                        textOf = state::textOf,
                        onLongPress = { actionsFor = it },
                        onReact = onReact,
                        isLoadingOlder = state.isLoadingOlder,
                        onNearOldest = { if (state.hasOlder) onLoadOlder() },
                        onOpenThread = onOpenThread,
                    )
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
            Composer(
                placeholder = stringResource(R.string.message_placeholder, title),
                draft = draft.text,
                isEditing = draft.editingId != null,
                onCancelEdit = draft::stopEdit,
                onSubmit = { scope.launch { draft.submit(onSend, onEdit) } },
            )
        }
    }
    actionsFor?.let { message ->
        MessageActions(
            message = message,
            onDismiss = { actionsFor = null },
            onReact = { emoji, active -> onReact(message.id, emoji, active) },
            onReplyInThread = { onOpenThread(message) },
            // The markdown as written, as the desktop copies it.
            onCopy = { scope.launch { clipboard.setClipEntry(sensitiveClipOf(message.text)) } },
            onEdit = { draft.startEdit(message) },
            onDelete = { deleting = message },
        )
    }
    deleting?.let { message ->
        ConfirmDelete(
            message = message,
            shownText = state.textOf(message),
            onConfirm = {
                onDelete(message.id)
                deleting = null
            },
            onDismiss = { deleting = null },
        )
    }
}

@Composable
private fun EmptyChannel() {
    Box(modifier = Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
        Text(stringResource(R.string.no_messages), color = LocalShyColors.current.palette.textMuted)
    }
}

@Composable
private fun MessageList(
    newestFirst: List<Message>,
    listState: LazyListState,
    textOf: (Message) -> String,
    onLongPress: (Message) -> Unit,
    onReact: (message: String, emoji: String, active: Boolean) -> Unit,
    isLoadingOlder: Boolean,
    onNearOldest: () -> Unit,
    onOpenThread: (Message) -> Unit,
) {
    val clock = rememberClock()
    val rows = remember(newestFirst, clock.zone) { channelRows(newestFirst, clock.zone) }
    // A few rows from the oldest loaded, the next page starts loading, before the end is reached.
    val currentOnNearOldest by rememberUpdatedState(onNearOldest)
    LaunchedEffect(listState) {
        snapshotFlow {
            val info = listState.layoutInfo
            info.totalItemsCount > 0 && (info.visibleItemsInfo.lastOrNull()?.index ?: 0) >= info.totalItemsCount - OLDER_AHEAD
        }.collect { isNearOldest -> if (isNearOldest) currentOnNearOldest() }
    }
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
                is ChannelRow.Item -> MessageItem(row.message, textOf(row.message), row.isContinuation, clock, onLongPress, onReact, onOpenThread)
            }
        }
        if (isLoadingOlder) {
            item(key = "loading-older") {
                Box(modifier = Modifier.fillMaxWidth().padding(12.dp), contentAlignment = Alignment.Center) {
                    val loadingOlder = stringResource(R.string.loading_older)
                    CircularProgressIndicator(
                        modifier = Modifier.size(24.dp).semantics { contentDescription = loadingOlder },
                        strokeWidth = 2.dp,
                    )
                }
            }
        }
    }
}

/** How many rows before the oldest loaded one the next page starts loading. */
private const val OLDER_AHEAD = 10
