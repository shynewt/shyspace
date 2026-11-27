package dev.shynewt.shyspace.core

import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.channels.awaitClose
import kotlinx.coroutines.flow.buffer
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.callbackFlow
import uniffi.shy_core.Change
import uniffi.shy_ffi.ChangeListener
import uniffi.shy_ffi.ShyApp
import uniffi.shy_ffi.ShyWorkspace

/** The core for the whole app: one per process, since a workspace's folder can be open only once. */
class ShyCore(val app: ShyApp) {
    /**
     * The workspace [workspaceId], opening this phone's workspaces first if nothing has yet, as when
     * Android restores the app straight onto a screen. Null after telling [onFailure] why not, in
     * short: Home shows a failed open's full reason, which can name the data folder.
     */
    suspend fun openWorkspace(workspaceId: String, onFailure: (String) -> Unit): ShyWorkspace? {
        app.workspace(workspaceId)?.let { return it }
        val failures = coreCall(onFailure) { app.openAll() } ?: return null
        app.workspace(workspaceId)?.let { return it }
        val hasFailed = failures.any { it.workspaceId == workspaceId }
        onFailure(if (hasFailed) "This workspace couldn't open. Home shows why." else "This workspace isn't on this phone.")
        return null
    }

    /**
     * Every change in [workspace] while collected, none dropped when the collector is slow. It
     * starts with [Change.Sync] once listening: load on that, not before collecting, and no
     * change can fall between the load and the first change heard.
     */
    fun changes(workspace: ShyWorkspace): Flow<Change> = callbackFlow {
        val subscription = workspace.subscribe(
            object : ChangeListener {
                override fun onChange(change: Change) {
                    trySend(change)
                }
            },
        )
        trySend(Change.Sync)
        awaitClose { subscription.close() }
    }.buffer(Channel.UNLIMITED)
}
