package dev.shynewt.shyspace.features.channel

import androidx.compose.ui.semantics.paneTitle
import androidx.compose.ui.platform.ClipEntry
import android.os.PersistableBundle
import android.os.Build
import android.content.ClipDescription
import android.content.ClipData
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Icon
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.shynewt.shyspace.R
import dev.shynewt.shyspace.ui.theme.LocalShyColors
import uniffi.shy_core.Message

/** The desktop's quick reactions, in its order. */
val QUICK_REACTIONS = listOf("👍", "❤️", "😂", "🎉", "👀", "🚀")

/**
 * What can be done with [message]: react, reply in its thread (null inside the thread), copy,
 * and for its author, edit or delete.
 */
@Composable
fun MessageActions(
    message: Message,
    onDismiss: () -> Unit,
    onReact: (emoji: String, active: Boolean) -> Unit,
    onReplyInThread: (() -> Unit)?,
    onCopy: () -> Unit,
    onEdit: () -> Unit,
    onDelete: () -> Unit,
) {
    val colors = LocalShyColors.current
    ModalBottomSheet(onDismissRequest = onDismiss, containerColor = colors.palette.surface) {
        val sheetTitle = stringResource(R.string.message_actions)
        Column(modifier = Modifier.navigationBarsPadding().padding(bottom = 8.dp).semantics { paneTitle = sheetTitle }) {
            Row(
                horizontalArrangement = Arrangement.SpaceEvenly,
                modifier = Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 4.dp),
            ) {
                QUICK_REACTIONS.forEach { emoji ->
                    val isMine = message.reactions.any { it.emoji == emoji && it.mine }
                    val label = stringResource(if (isMine) R.string.remove_reaction else R.string.add_reaction, emoji)
                    Box(
                        contentAlignment = Alignment.Center,
                        modifier = Modifier
                            .size(48.dp)
                            .clip(CircleShape)
                            .background(if (isMine) colors.accent.accentSoft else colors.palette.surface2)
                            .semantics {
                                contentDescription = label
                                selected = isMine
                            }
                            .clickable(role = Role.Button) {
                                onReact(emoji, !isMine)
                                onDismiss()
                            },
                    ) { Text(emoji, fontSize = 22.sp) }
                }
            }
            if (onReplyInThread != null) {
                Action(R.drawable.ic_reply, R.string.reply_in_thread) {
                    onReplyInThread()
                    onDismiss()
                }
            }
            Action(R.drawable.ic_copy, R.string.copy_text) {
                onCopy()
                onDismiss()
            }
            if (message.canEdit) {
                Action(R.drawable.ic_edit, R.string.edit) {
                    onEdit()
                    onDismiss()
                }
            }
            if (message.canDelete) {
                Action(R.drawable.ic_delete, R.string.delete, tint = colors.palette.dangerText) {
                    onDelete()
                    onDismiss()
                }
            }
        }
    }
}

@Composable
private fun Action(icon: Int, label: Int, tint: Color = Color.Unspecified, onClick: () -> Unit) {
    val colors = LocalShyColors.current.palette
    ListItem(
        leadingContent = { Icon(painterResource(icon), contentDescription = null, tint = if (tint == Color.Unspecified) colors.textMuted else tint) },
        colors = ListItemDefaults.colors(containerColor = Color.Transparent),
        modifier = Modifier.fillMaxWidth().clickable(role = Role.Button, onClick = onClick),
    ) {
        Text(stringResource(label), color = if (tint == Color.Unspecified) colors.text else tint)
    }
}

/** Asks before deleting [message], quoting it as [shownText], as the desktop does. */
@Composable
fun ConfirmDelete(message: Message, shownText: String, onConfirm: () -> Unit, onDismiss: () -> Unit) {
    val colors = LocalShyColors.current.palette
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.delete_message_title)) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                Text(
                    stringResource(
                        if (message.files.isEmpty()) R.string.delete_message_detail else R.string.delete_message_with_files_detail,
                    ),
                )
                Text(
                    shownText,
                    maxLines = 3,
                    overflow = TextOverflow.Ellipsis,
                    style = MaterialTheme.typography.bodyMedium,
                    color = colors.textMuted,
                )
            }
        },
        confirmButton = {
            TextButton(onClick = onConfirm) { Text(stringResource(R.string.delete_message), color = colors.dangerText) }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.cancel)) } },
    )
}

/**
 * [text] for the clipboard, marked sensitive: the system's paste preview shows dots instead of a
 * private message. Pasting works as usual.
 */
fun sensitiveClipOf(text: String): ClipEntry {
    val clip = ClipData.newPlainText("message", text)
    clip.description.extras = PersistableBundle().apply { putBoolean(IS_SENSITIVE, true) }
    return ClipEntry(clip)
}

// The key is public from API 33 but older versions read the same string.
private val IS_SENSITIVE =
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) ClipDescription.EXTRA_IS_SENSITIVE else "android.content.extra.IS_SENSITIVE"
