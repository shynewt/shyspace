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
    val isLoading: Boolean = true,
    val error: String? = null,
)

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

    private suspend fun follow() {
        if (workspace == null) {
            mutableState.value = ChannelState(isLoading = false, error = "This workspace isn't open.")
            return
        }
        reload(workspace)
        core.changes(workspace).collect { change ->
            if ((change is Change.Messages && change.channel == channelId) || change is Change.Sync) reload(workspace)
        }
    }

    private suspend fun reload(workspace: ShyWorkspace) {
        val messages = coreCall(::showError) { workspace.messages(channelId, null, PAGE) } ?: return
        mutableState.value = ChannelState(messages = messages.asReversed(), isLoading = false)
    }

    private fun showError(reason: String) {
        mutableState.update { it.copy(isLoading = false, error = reason) }
    }

    private companion object {
        const val PAGE = 50u
    }
}
