package dev.shynewt.shyspace.features.channel

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import dev.shynewt.shyspace.core.ShyCore
import dev.shynewt.shyspace.core.WorkspaceHandle
import dev.shynewt.shyspace.core.coreCall
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.sync.Mutex
import uniffi.shy_core.Change
import uniffi.shy_core.Message
import uniffi.shy_ffi.ShyWorkspace

data class ChannelState(
    /** Newest first, as the list shows them from the bottom up. */
    val messages: List<Message> = emptyList(),
    /** Each message's text as shown, by id: app links named, no markdown marks, lines and list items kept. */
    val plainTexts: Map<String, String> = emptyMap(),
    val isLoading: Boolean = true,
    /** Whether older messages may be left to load: the last load came back full. */
    val hasOlder: Boolean = false,
    val isLoadingOlder: Boolean = false,
    /** A linked phone that can read here but not send until the workspace adds it. */
    val isReadingOnly: Boolean = false,
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
    // Released when the screen goes: the core needs every handle gone to swap a phone's key when linking.
    private val followed = WorkspaceHandle(core, workspaceId, ::showError).also(::addCloseable)

    /** How many of the newest messages to show; loading older ones raises it a page at a time. */
    private var shownLimit = PAGE
    private val reloading = Mutex()

    init {
        viewModelScope.launch { follow() }
    }

    /** Sends [text], trimmed; false when there was nothing to send or sending failed. */
    suspend fun send(text: String): Boolean {
        val trimmed = text.trim()
        if (trimmed.isEmpty()) return false
        val workspace = followed.get() ?: return false
        return coreCall(::showError) { workspace.sendMessage(channelId, trimmed, null) } != null
    }

    /** Replaces a message's text with [text], trimmed; false when it's blank or the edit failed. */
    suspend fun edit(message: String, text: String): Boolean {
        val trimmed = text.trim()
        if (trimmed.isEmpty()) return false
        val workspace = followed.get() ?: return false
        return coreCall(::showError) { workspace.editMessage(message, trimmed) } != null
    }

    fun react(message: String, emoji: String, active: Boolean) {
        viewModelScope.launch {
            val workspace = followed.get() ?: return@launch
            coreCall(::showError) { workspace.react(message, emoji, active) }
        }
    }

    fun delete(message: String) {
        viewModelScope.launch {
            val workspace = followed.get() ?: return@launch
            coreCall(::showError) { workspace.deleteMessage(message) }
        }
    }

    /** Loads the page of messages just older than the oldest shown. */
    fun loadOlder() {
        val current = mutableState.value
        if (!current.hasOlder || current.isLoadingOlder) return
        mutableState.update { it.copy(isLoadingOlder = true) }
        shownLimit += PAGE
        viewModelScope.launch {
            val workspace = followed.get() ?: return@launch
            reload(workspace)
        }
    }

    /** You saw [message], the newest: the channel is read up to it. */
    fun markSeen(message: String) {
        viewModelScope.launch {
            val workspace = followed.get() ?: return@launch
            coreCall(::showError) { workspace.markRead(channelId, message) }
        }
    }

    private suspend fun follow() {
        followed.changes().collect { change ->
            if ((change is Change.Messages && change.channel == channelId) || change is Change.Sync) {
                reload(followed.get() ?: return@collect)
            }
        }
    }

    /**
     * Shows the newest [shownLimit] messages. The only writer of the list, one at a time, so a
     * change arriving while older ones load can neither drop the page nor leave a gap.
     */
    private suspend fun reload(workspace: ShyWorkspace) = reloading.withLock {
        val limit = shownLimit
        val messages = coreCall(::showError) { workspace.messages(channelId, null, limit) }
        val plain = messages?.let { coreCall(::showError) { workspace.plainTexts(it.map { message -> message.text }) } }
        if (messages == null || plain == null) {
            mutableState.update { it.copy(isLoadingOlder = false) }
            return@withLock
        }
        mutableState.value = ChannelState(
            messages = messages.asReversed(),
            plainTexts = messages.map { it.id }.zip(plain).toMap(),
            isLoading = false,
            // A short load reached the channel's start; one that grew later fills the page again.
            hasOlder = messages.size.toUInt() == limit,
            // Older ones asked for after this load began are still on their way.
            isLoadingOlder = shownLimit > limit,
            isReadingOnly = workspace.isReadingOnly(),
        )
    }

    private fun showError(reason: String) {
        mutableState.update { it.copy(isLoading = false, error = reason) }
    }

    private companion object {
        const val PAGE = 50u
    }
}
