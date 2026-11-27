package dev.shynewt.shyspace.features.activity

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import dev.shynewt.shyspace.core.ShyCore
import dev.shynewt.shyspace.core.WorkspaceHandle
import dev.shynewt.shyspace.core.coreCall
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import uniffi.shy_core.Change
import uniffi.shy_core.Channel
import uniffi.shy_core.InboxItem
import uniffi.shy_ffi.ShyWorkspace

data class ActivityState(
    val workspaceId: String? = null,
    /** Mentions of you and new replies in threads you follow, newest first. */
    val attention: List<InboxItem> = emptyList(),
    /** Channels with unread messages; the core leaves out muted ones unless they mention you. */
    val unreadChannels: List<Channel> = emptyList(),
    /** Each attention message's text as shown, by message id. */
    val previews: Map<String, String> = emptyMap(),
    val isLoading: Boolean = true,
    val error: String? = null,
)

/** What needs your attention in one workspace, kept current. */
class ActivityViewModel(private val core: ShyCore) : ViewModel() {
    private val mutableState = MutableStateFlow(ActivityState())
    val state: StateFlow<ActivityState> = mutableState.asStateFlow()
    private var following: Job? = null
    /** The workspace followed, released when another replaces it or this ends. */
    private var followed: WorkspaceHandle? = null

    /** Shows [workspaceId]'s activity from now on. */
    fun follow(workspaceId: String) {
        if (mutableState.value.workspaceId == workspaceId && following?.isActive == true) return
        stopFollowing()
        mutableState.value = ActivityState(workspaceId = workspaceId)
        val handle = WorkspaceHandle(core, workspaceId, ::showError)
        followed = handle
        following = viewModelScope.launch {
            handle.changes().collect { change ->
                if (change is Change.Messages || change is Change.Channels || change is Change.Sync) {
                    reload(handle.get() ?: return@collect)
                }
            }
        }
    }

    /** There's no workspace to follow, for [reason]: say so instead of loading. */
    fun showUnavailable(reason: String) {
        stopFollowing()
        mutableState.value = ActivityState(isLoading = false, error = reason)
    }

    /** Marks [channelId] read up to its newest message; a failure is shown above the list, which stays. */
    fun markChannelRead(channelId: String) {
        val handle = followed ?: return
        viewModelScope.launch {
            val workspace = handle.get() ?: return@launch
            coreCall(::showError) { workspace.markChannelRead(channelId) }
        }
    }

    private suspend fun reload(workspace: ShyWorkspace) {
        val inbox = coreCall(::showError) { workspace.inbox() } ?: return
        val previews = coreCall(::showError) { workspace.plainTexts(inbox.attention.map { it.message.text }) } ?: return
        mutableState.value = ActivityState(
            workspaceId = workspace.workspaceId(),
            attention = inbox.attention,
            unreadChannels = inbox.channels,
            previews = inbox.attention.map { it.message.id }.zip(previews).toMap(),
            isLoading = false,
        )
    }

    override fun onCleared() = stopFollowing()

    private fun stopFollowing() {
        following?.cancel()
        followed?.close()
        followed = null
    }

    private fun showError(reason: String) {
        mutableState.update { it.copy(isLoading = false, error = reason) }
    }
}
