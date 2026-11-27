package dev.shynewt.shyspace.features.home

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import dev.shynewt.shyspace.core.ShyCore
import dev.shynewt.shyspace.core.coreCall
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import uniffi.shy_core.Change
import uniffi.shy_core.Channel
import uniffi.shy_core.Member
import uniffi.shy_ffi.BucketSettings
import uniffi.shy_ffi.OpenFailure
import uniffi.shy_ffi.ShyWorkspace

sealed interface HomeState {
    data object Loading : HomeState

    /** Nothing on this phone yet; [createError] says why the last create failed. */
    data class NoWorkspace(val isCreating: Boolean = false, val createError: String? = null) : HomeState

    data class Ready(
        val workspaceId: String,
        val workspaceName: String,
        val channels: List<Channel>,
        /** Everyone in it, for direct messages' names. */
        val members: List<Member> = emptyList(),
        /** Workspaces here that couldn't open, and why. */
        val openFailures: List<OpenFailure>,
    ) : HomeState

    data class Failed(val reason: String) : HomeState
}

/** The device's first workspace and its channels, kept current. */
class HomeViewModel(private val core: ShyCore) : ViewModel() {
    private val mutableState = MutableStateFlow<HomeState>(HomeState.Loading)
    val state: StateFlow<HomeState> = mutableState.asStateFlow()

    init {
        viewModelScope.launch { open() }
    }

    /** Creates a workspace on the given bucket, for development builds. */
    fun createWorkspace(workspaceName: String, memberName: String, bucket: BucketSettings) {
        mutableState.value = HomeState.NoWorkspace(isCreating = true)
        viewModelScope.launch {
            val workspace = coreCall({ reason -> mutableState.value = HomeState.NoWorkspace(createError = reason) }) {
                core.app.createWorkspace(workspaceName, memberName, bucket)
            } ?: return@launch
            show(workspace, emptyList())
        }
    }

    /** Opens this phone's workspaces again, after they failed to. */
    fun retry() {
        mutableState.value = HomeState.Loading
        viewModelScope.launch { open() }
    }

    private suspend fun open() {
        val failures = coreCall(::fail) { core.app.openAll() } ?: return
        val listed = coreCall(::fail) { core.app.workspaces() } ?: return
        val first = listed.firstOrNull()?.let { core.app.workspace(it.workspaceId) }
        if (first == null) {
            mutableState.value = if (failures.isEmpty()) HomeState.NoWorkspace() else HomeState.Failed(failures.describe())
            return
        }
        show(first, failures)
    }

    private suspend fun show(workspace: ShyWorkspace, failures: List<OpenFailure>) {
        core.changes(workspace).collect { change ->
            if (change is Change.Channels || change is Change.Messages || change is Change.Members || change is Change.Sync) {
                val channels = coreCall(::fail) { workspace.channels() } ?: return@collect
                val members = coreCall(::fail) { workspace.members() } ?: return@collect
                mutableState.value = HomeState.Ready(
                    workspaceId = workspace.workspaceId(),
                    workspaceName = workspace.name(),
                    channels = channels,
                    members = members,
                    openFailures = failures,
                )
            }
        }
    }

    private fun fail(reason: String) {
        mutableState.value = HomeState.Failed(reason)
    }

    private fun List<OpenFailure>.describe() = joinToString("\n") { "${it.workspaceId}: ${it.reason}" }
}
