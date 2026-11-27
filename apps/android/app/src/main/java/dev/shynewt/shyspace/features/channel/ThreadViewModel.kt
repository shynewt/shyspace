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

data class ThreadState(
    /** The message the thread started from. */
    val root: Message? = null,
    /** Its replies, newest first, as the list shows them from the bottom up. */
    val replies: List<Message> = emptyList(),
    /** Each message's text as shown, by id. */
    val plainTexts: Map<String, String> = emptyMap(),
    val isLoading: Boolean = true,
    /** Its first message was deleted, and the thread with it. */
    val isGone: Boolean = false,
    val error: String? = null,
) {
    fun textOf(message: Message): String = plainTexts[message.id] ?: message.text
}

/** One thread, kept current, and replying in it. */
class ThreadViewModel(
    private val core: ShyCore,
    workspaceId: String,
    private val channelId: String,
    private val rootId: String,
) : ViewModel() {
    private val mutableState = MutableStateFlow(ThreadState())
    val state: StateFlow<ThreadState> = mutableState.asStateFlow()
    private val workspace: ShyWorkspace? = core.app.workspace(workspaceId)

    init {
        viewModelScope.launch { follow() }
    }

    /** Replies with [text], trimmed; false when there was nothing to send or sending failed. */
    suspend fun reply(text: String): Boolean {
        val trimmed = text.trim()
        if (trimmed.isEmpty() || workspace == null) return false
        return coreCall(::showError) { workspace.sendMessage(channelId, trimmed, rootId) } != null
    }

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

    /** You saw [message], the newest reply: the thread is read up to it. */
    fun markSeen(message: String) {
        if (workspace == null) return
        viewModelScope.launch { coreCall(::showError) { workspace.markThreadRead(rootId, message) } }
    }

    private suspend fun follow() {
        if (workspace == null) {
            mutableState.value = ThreadState(isLoading = false, error = "This workspace isn't open.")
            return
        }
        core.changes(workspace).collect { change ->
            if ((change is Change.Messages && change.channel == channelId) || change is Change.Sync) reload(workspace)
        }
    }

    private suspend fun reload(workspace: ShyWorkspace) {
        var failure: String? = null
        val thread = coreCall({ failure = it }) { workspace.thread(rootId) }
        if (thread == null) {
            val isRootThere = coreCall(::showError) { workspace.linkedMessage(rootId) != null } ?: return
            if (isRootThere) {
                showError(checkNotNull(failure) { "coreCall gives null only after reporting a failure" })
            } else {
                mutableState.value = ThreadState(isLoading = false, isGone = true)
            }
            return
        }
        val plain = coreCall(::showError) { workspace.plainTexts(thread.map { it.text }) } ?: return
        mutableState.value = ThreadState(
            root = thread.firstOrNull(),
            replies = thread.drop(1).asReversed(),
            plainTexts = thread.map { it.id }.zip(plain).toMap(),
            isLoading = false,
        )
    }

    private fun showError(reason: String) {
        mutableState.update { it.copy(isLoading = false, error = reason) }
    }
}
