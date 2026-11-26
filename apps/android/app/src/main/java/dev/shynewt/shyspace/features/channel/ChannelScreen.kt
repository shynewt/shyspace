package dev.shynewt.shyspace.features.channel

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextField
import androidx.compose.material3.TextFieldDefaults
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import dev.shynewt.shyspace.R
import dev.shynewt.shyspace.ui.theme.LocalShyColors
import java.text.DateFormat
import java.util.Date
import kotlinx.coroutines.launch
import uniffi.shy_core.Message

/** A channel: its messages, newest at the bottom, and a composer that rides on the keyboard. */
@Composable
fun ChannelScreen(
    title: String,
    state: ChannelState,
    onBack: () -> Unit,
    onSend: suspend (String) -> Boolean,
) {
    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(title, modifier = Modifier.semantics { heading() }) },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(painterResource(R.drawable.ic_arrow_back), contentDescription = stringResource(R.string.back))
                    }
                },
            )
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
                    else -> MessageList(state.messages)
                }
            }
            state.error?.let { error ->
                Text(
                    error,
                    color = LocalShyColors.current.palette.dangerText,
                    style = MaterialTheme.typography.bodySmall,
                    modifier = Modifier.padding(horizontal = 16.dp, vertical = 4.dp),
                )
            }
            Composer(title = title, onSend = onSend)
        }
    }
}

@Composable
private fun EmptyChannel() {
    Box(modifier = Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
        Text(stringResource(R.string.no_messages), color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

@Composable
private fun MessageList(messages: List<Message>) {
    // Reversed, so the newest sits at the bottom and new ones keep the view there.
    LazyColumn(
        reverseLayout = true,
        contentPadding = PaddingValues(horizontal = 16.dp, vertical = 8.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
        modifier = Modifier.fillMaxSize(),
    ) {
        items(messages, key = { it.id }) { message -> MessageRow(message) }
    }
}

@Composable
private fun MessageRow(message: Message) {
    val faint = LocalShyColors.current.palette.textFaint
    Column(modifier = Modifier.fillMaxWidth().semantics(mergeDescendants = true) {}) {
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.Bottom) {
            Text(message.authorName, style = MaterialTheme.typography.titleSmall, fontWeight = FontWeight.SemiBold)
            Text(
                DateFormat.getTimeInstance(DateFormat.SHORT).format(Date(message.createdMs)),
                style = MaterialTheme.typography.labelSmall,
                color = faint,
            )
        }
        Text(message.text, style = MaterialTheme.typography.bodyLarge)
    }
}

@Composable
private fun Composer(title: String, onSend: suspend (String) -> Boolean) {
    var draft by rememberSaveable { mutableStateOf("") }
    val scope = rememberCoroutineScope()
    val send = {
        val text = draft
        scope.launch { if (onSend(text)) draft = "" }
        Unit
    }
    Row(
        verticalAlignment = Alignment.CenterVertically,
        modifier = Modifier
            .fillMaxWidth()
            .navigationBarsPadding()
            .padding(horizontal = 8.dp, vertical = 8.dp),
    ) {
        TextField(
            value = draft,
            onValueChange = { draft = it },
            placeholder = { Text(stringResource(R.string.message_placeholder, title)) },
            shape = MaterialTheme.shapes.large,
            colors = TextFieldDefaults.colors(
                focusedIndicatorColor = Color.Transparent,
                unfocusedIndicatorColor = Color.Transparent,
                disabledIndicatorColor = Color.Transparent,
            ),
            modifier = Modifier.weight(1f),
        )
        IconButton(onClick = send, enabled = draft.isNotBlank()) {
            Icon(
                painterResource(R.drawable.ic_send),
                contentDescription = stringResource(R.string.send),
                tint = if (draft.isNotBlank()) MaterialTheme.colorScheme.primary else LocalShyColors.current.palette.textDecorative,
            )
        }
    }
}
