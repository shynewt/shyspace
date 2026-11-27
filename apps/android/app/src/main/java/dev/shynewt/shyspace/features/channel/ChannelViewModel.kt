package dev.shynewt.shyspace.features.channel

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import dev.shynewt.shyspace.core.ShyCore
import dev.shynewt.shyspace.core.coreCall
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import uniffi.shy_core.Change
import uniffi.shy_core.Message
import uniffi.shy_ffi.ShyWorkspace

data class ChannelState(
    /** Newest first, as the list shows them from the bottom up. */
    val messages: List<Message> = emptyList(),
    /** Each message's text as shown, by id: app links named, no markdown marks, lines and list items kept. */
    val plainTexts: Map<String, String> = emptyMap(),
    val isLoading: Boolean = true,
    val error: String? = null,
) {
    /** A message's plain text, or its own where none is known (screens drawn without a core). */
    fun textOf(message: Message): String = plainTexts[message.id] ?: message.text
}

/** One channel's latest messages, kept current, and sending into it. */
class ChannelViewModel(
    private val core: ShyCore,
    workspaceId: String,
    private val channelId: String,
) : ViewModel() {
    private val mutableState = MutableStateFlow(ChannelState())
    val state: StateFlow<ChannelState> = mutableState.asStateFlow()
    private val workspace: ShyWorkspace? = core.app.workspace(workspaceId)

    init {
        viewModelScope.launch { follow() }
    }

    /** Sends [text], trimmed; false when there was nothing to send or sending failed. */
    suspend fun send(text: String): Boolean {
        val trimmed = text.trim()
        if (trimmed.isEmpty() || workspace == null) return false
        return coreCall(::showError) { workspace.sendMessage(channelId, trimmed, null) } != null
    }

    /** Replaces a message's text with [text], trimmed; false when it's blank or the edit failed. */
    suspend fun edit(message: String, text: String): Boolean {
        val trimmed = text.trim()
        if (trimmed.isEmpty() || workspace == null) return false
        return coreCall(::showError) { workspace.editMessage(message, trimmed) } != null
    }

    fun react(message: String, emoji: String, active: Boolean) {
        if (workspace == null) return
        viewModelScope.launch { coreCall(::showError) { workspace.react(message, emoji, active) } }
    }

    fun delete(message: String) {
        if (workspace == null) return
        viewModelScope.launch { coreCall(::showError) { workspace.deleteMessage(message) } }
    }

    /** You saw [message], the newest: the channel is read up to it. */
    fun markSeen(message: String) {
        if (workspace == null) return
        viewModelScope.launch { coreCall(::showError) { workspace.markRead(channelId, message) } }
    }

    private suspend fun follow() {
        if (workspace == null) {
            mutableState.value = ChannelState(isLoading = false, error = "This workspace isn't open.")
            return
        }
        core.changes(workspace).collect { change ->
            if ((change is Change.Messages && change.channel == channelId) || change is Change.Sync) reload(workspace)
        }
    }

    private suspend fun reload(workspace: ShyWorkspace) {
        val messages = coreCall(::showError) { workspace.messages(channelId, null, PAGE) } ?: return
        val plain = coreCall(::showError) { workspace.plainTexts(messages.map { it.text }) } ?: return
        mutableState.value = ChannelState(
            messages = messages.asReversed(),
            plainTexts = messages.map { it.id }.zip(plain).toMap(),
            isLoading = false,
        )
    }

    private fun showError(reason: String) {
        mutableState.update { it.copy(isLoading = false, error = reason) }
    }

    private companion object {
        const val PAGE = 50u
    }
}
