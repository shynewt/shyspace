package dev.shynewt.shyspace.core

import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.flow
import kotlinx.coroutines.flow.takeWhile
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import uniffi.shy_core.Change
import uniffi.shy_ffi.ShyWorkspace

/**
 * One workspace a screen follows, by id: opened when first needed, and released by [close]. When a
 * linked phone's reading workspace gives way to the joined one (Change.ReadingClosed), the handle
 * is swapped for the joined workspace's and its changes carry on. [changesOf] is the core's
 * change stream for a handle.
 */
class WorkspaceHandle(
    private val core: ShyCore,
    private val workspaceId: String,
    private val onFailure: (String) -> Unit,
    private val changesOf: (ShyWorkspace) -> Flow<Change> = core::changes,
) : AutoCloseable {
    private val lock = Mutex()
    private var handle: ShyWorkspace? = null
    private var hasOpened = false

    /** The workspace now, opened on first use; null after telling onFailure why it can't be. */
    suspend fun get(): ShyWorkspace? = lock.withLock {
        handle ?: if (hasOpened) {
            null
        } else {
            hasOpened = true
            core.openWorkspace(workspaceId, onFailure).also { handle = it }
        }
    }

    /** Its changes, each run starting with Change.Sync, including a new run after the swap. */
    fun changes(): Flow<Change> = flow {
        while (true) {
            val workspace = get() ?: return@flow
            var isReadingClosed = false
            changesOf(workspace)
                .takeWhile { change -> (change != Change.ReadingClosed).also { goesOn -> isReadingClosed = !goesOn } }
                .collect { change -> emit(change) }
            if (!isReadingClosed) return@flow
            swapForTheJoinedOne()
        }
    }

    private suspend fun swapForTheJoinedOne() = lock.withLock {
        handle?.close()
        // The core puts the joined workspace in place before it says the reading one closed.
        handle = core.app.workspace(workspaceId)
        if (handle == null) onFailure("This workspace closed. Open it again from Home.")
    }

    override fun close() {
        handle?.close()
        handle = null
    }
}
