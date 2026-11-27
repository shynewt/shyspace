package dev.shynewt.shyspace.features.onboarding

import dev.shynewt.shyspace.core.coreCall
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch

/**
 * The part of linking that outlives the link screen, for the app's whole life ([scope]): the wait
 * until the workspace adds this phone, which can take a while, and a link a restart interrupted.
 */
class LinkKeeper(private val linking: PhoneLinking, private val scope: CoroutineScope) {
    private val mutableProblem = MutableStateFlow<String?>(null)

    /** Why the last link ended without this phone being added, in the core's words. */
    val problem: StateFlow<String?> = mutableProblem.asStateFlow()
    private var waiting: Job? = null

    /** After the hand-over: waits here until the workspace adds this phone, and the joined one takes over. */
    fun waitUntilAdded() {
        if (waiting?.isActive == true) return
        mutableProblem.value = null
        waiting = scope.launch { coreCall({ reason -> mutableProblem.value = reason }) { linking.waitUntilLinked() } }
    }

    /**
     * After the workspaces opened: picks up a link a restart interrupted. One reading goes on
     * waiting here; one still waiting for its hand-over is for the link screen to show.
     */
    suspend fun resume(): PickedUpLink? {
        val picked = coreCall({ reason -> mutableProblem.value = reason }) { linking.resumeLink() }
        if (picked is PickedUpLink.Reading) waitUntilAdded()
        return picked
    }
}
