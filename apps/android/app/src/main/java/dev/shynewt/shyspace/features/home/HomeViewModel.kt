package dev.shynewt.shyspace.features.home

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import dev.shynewt.shyspace.core.ShyCore
import dev.shynewt.shyspace.core.WorkspaceHandle
import dev.shynewt.shyspace.core.coreCall
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import uniffi.shy_core.Change
import uniffi.shy_core.Channel
import uniffi.shy_core.Member
import uniffi.shy_ffi.BucketSettings
import uniffi.shy_ffi.OpenFailure

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
    /** The handle on the workspace shown, released when another replaces it or Home ends. */
    private var shown: WorkspaceHandle? = null
    /** Keeps the shown workspace current; stopped before its handle is released. */
    private var showing: Job? = null

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
            val workspaceId = workspace.workspaceId()
            workspace.close()
            show(workspaceId, emptyList())
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
        val first = listed.firstOrNull()?.workspaceId?.takeIf { id -> failures.none { it.workspaceId == id } }
        if (first == null) {
            mutableState.value = if (failures.isEmpty()) HomeState.NoWorkspace() else HomeState.Failed(failures.describe())
            return
        }
        show(first, failures)
    }

    private fun show(workspaceId: String, failures: List<OpenFailure>) {
        stopShowing()
        val handle = WorkspaceHandle(core, workspaceId, ::fail)
        shown = handle
        showing = viewModelScope.launch {
            handle.changes().collect { change ->
                if (change is Change.Channels || change is Change.Messages || change is Change.Members || change is Change.Sync) {
                    val workspace = handle.get() ?: return@collect
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
    }

    override fun onCleared() = stopShowing()

    private fun stopShowing() {
        showing?.cancel()
        shown?.close()
        shown = null
    }

    private fun fail(reason: String) {
        mutableState.value = HomeState.Failed(reason)
    }

    private fun List<OpenFailure>.describe() = joinToString("\n") { "${it.workspaceId}: ${it.reason}" }
}
