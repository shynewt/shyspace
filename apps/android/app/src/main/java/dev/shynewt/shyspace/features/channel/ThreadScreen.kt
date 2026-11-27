package dev.shynewt.shyspace.features.channel

import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.Alignment
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.platform.LocalClipboard
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.runtime.snapshotFlow
import kotlinx.coroutines.flow.first
import androidx.compose.runtime.LaunchedEffect
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.repeatOnLifecycle
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
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
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.pluralStringResource
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

/** A thread: its first message at the top, replies below it, and a composer that replies. */
@Composable
fun ThreadScreen(
    channelTitle: String,
    state: ThreadState,
    onBack: () -> Unit,
    onReply: suspend (String) -> Boolean,
    onSeen: (newestReply: String) -> Unit,
    onEdit: suspend (message: String, text: String) -> Boolean,
    onReact: (message: String, emoji: String, active: Boolean) -> Unit,
    onDelete: (message: String) -> Unit,
) {
    val scope = rememberCoroutineScope()
    val clipboard = LocalClipboard.current
    val draft = rememberMessageDraft()
    var actionsFor by remember { mutableStateOf<Message?>(null) }
    var deleting by remember { mutableStateOf<Message?>(null) }
    val listState = rememberLazyListState()
    val newest = state.replies.firstOrNull()?.id
    // As in a channel: read once the newest reply is in view and the screen is in front.
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    LaunchedEffect(newest, lifecycle) {
        if (newest == null) return@LaunchedEffect
        lifecycle.repeatOnLifecycle(Lifecycle.State.RESUMED) {
            snapshotFlow { listState.firstVisibleItemIndex == 0 }.first { it }
            onSeen(newest)
        }
    }
    LaunchedEffect(state, draft.editingId) {
        val editing = draft.editingId ?: return@LaunchedEffect
        val isShown = state.root?.id == editing || state.replies.any { it.id == editing }
        if (!state.isLoading && !isShown) draft.editedMessageGone()
    }
    val surface = LocalShyColors.current.palette.surface
    val clock = rememberClock()
    Scaffold(
        containerColor = surface,
        topBar = {
            Column {
                TopAppBar(
                    title = {
                        Column {
                            Text(
                                stringResource(R.string.thread),
                                style = MaterialTheme.typography.titleMedium,
                                fontWeight = FontWeight.SemiBold,
                                modifier = Modifier.semantics { heading() },
                            )
                            Text(
                                stringResource(R.string.thread_in, channelTitle),
                                style = MaterialTheme.typography.labelMedium,
                                color = LocalShyColors.current.palette.textMuted,
                            )
                        }
                    },
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
            val root = state.root
            when {
                state.isGone -> CalmNote(stringResource(R.string.thread_deleted), modifier = Modifier.weight(1f))
                root == null -> Box(modifier = Modifier.weight(1f).fillMaxWidth(), contentAlignment = Alignment.Center) {
                    if (state.isLoading) CircularProgressIndicator()
                }
                else -> {
                    val rows = remember(root, state.replies, clock.zone) { channelRows(state.replies + root, clock.zone) }
                    LazyColumn(
                        state = listState,
                        reverseLayout = true,
                        // A short thread reads from the top, its first message under the title.
                        verticalArrangement = Arrangement.Top,
                        contentPadding = PaddingValues(top = 8.dp, bottom = 12.dp),
                        modifier = Modifier.weight(1f).fillMaxWidth(),
                    ) {
                        items(rows, key = { it.key }) { row ->
                            when (row) {
                                is ChannelRow.Day -> DayDivider(dayLabel(row.date, clock.today), modifier = Modifier.padding(horizontal = GUTTER))
                                is ChannelRow.Item -> Column {
                                    MessageItem(row.message, state.textOf(row.message), row.isContinuation, clock, { actionsFor = it }, onReact, onOpenThread = null)
                                    val count = state.replies.size
                                    if (row.message.id == root.id && count > 0) {
                                        DayDivider(pluralStringResource(R.plurals.replies, count, count), modifier = Modifier.padding(horizontal = GUTTER))
                                    }
                                }
                            }
                        }
                    }
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
            if (!state.isGone) Composer(
                placeholder = stringResource(R.string.reply_placeholder),
                draft = draft.text,
                isEditing = draft.editingId != null,
                onCancelEdit = draft::stopEdit,
                onSubmit = { scope.launch { draft.submit(onReply, onEdit) } },
            )
        }
    }
    actionsFor?.let { message ->
        MessageActions(
            message = message,
            onDismiss = { actionsFor = null },
            onReact = { emoji, active -> onReact(message.id, emoji, active) },
            onReplyInThread = null,
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
private fun CalmNote(text: String, modifier: Modifier = Modifier) {
    Box(modifier = modifier.fillMaxWidth().padding(32.dp), contentAlignment = Alignment.Center) {
        Text(text, style = MaterialTheme.typography.bodyLarge, color = LocalShyColors.current.palette.textMuted, textAlign = TextAlign.Center)
    }
}
