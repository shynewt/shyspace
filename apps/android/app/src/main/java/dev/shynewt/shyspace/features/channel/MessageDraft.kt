package dev.shynewt.shyspace.features.channel

import androidx.compose.foundation.text.input.TextFieldState
import androidx.compose.foundation.text.input.clearText
import androidx.compose.foundation.text.input.setTextAndPlaceCursorAtEnd
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.listSaver
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import uniffi.shy_core.Message

/**
 * The composer's text, and the message being edited in it. Editing sets the unsent text aside
 * and brings it back when the edit ends, so nothing typed is lost.
 */
@Stable
class MessageDraft private constructor(
    val text: TextFieldState,
    editingId: String?,
    private var setAside: String,
) {
    var editingId by mutableStateOf(editingId)
        private set

    /** A send or save is on its way; another tap does nothing until it's done. */
    var isSending by mutableStateOf(false)
        private set

    fun startEdit(message: Message) {
        if (editingId == null) setAside = text.text.toString()
        editingId = message.id
        text.setTextAndPlaceCursorAtEnd(message.text)
    }

    fun stopEdit() {
        editingId = null
        text.setTextAndPlaceCursorAtEnd(setAside)
        setAside = ""
    }

    /** The message being edited is gone: stop editing, keeping both the unsent and the edited text. */
    fun editedMessageGone() {
        val edited = text.text.toString()
        editingId = null
        text.setTextAndPlaceCursorAtEnd(listOf(setAside, edited).filter { it.isNotBlank() }.joinToString("\n\n"))
        setAside = ""
    }

    /** Sends the text, or saves the edit; on failure everything stays as it was. */
    suspend fun submit(send: suspend (String) -> Boolean, edit: suspend (message: String, text: String) -> Boolean) {
        if (isSending) return
        isSending = true
        try {
            val editing = editingId
            val body = text.text.toString()
            val isDone = if (editing == null) send(body) else edit(editing, body)
            if (!isDone) return
            if (editing == null) text.clearText() else stopEdit()
        } finally {
            isSending = false
        }
    }

    companion object {
        fun empty(): MessageDraft = MessageDraft(TextFieldState(), editingId = null, setAside = "")

        val Saver = listSaver<MessageDraft, String?>(
            save = { listOf(it.text.text.toString(), it.editingId, it.setAside) },
            restore = { MessageDraft(TextFieldState(it[0].orEmpty()), it[1], it[2].orEmpty()) },
        )
    }
}

@Composable
fun rememberMessageDraft(): MessageDraft = rememberSaveable(saver = MessageDraft.Saver) {
    MessageDraft.empty()
}
