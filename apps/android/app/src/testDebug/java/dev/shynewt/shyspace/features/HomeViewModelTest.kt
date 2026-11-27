package dev.shynewt.shyspace.features

import dev.shynewt.shyspace.MainThreadRule
import dev.shynewt.shyspace.createAcme
import dev.shynewt.shyspace.features.home.HomeState
import dev.shynewt.shyspace.features.home.HomeViewModel
import dev.shynewt.shyspace.testCore
import dev.shynewt.shyspace.unusedBucket
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.delay
import kotlinx.coroutines.withTimeout
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder

class HomeViewModelTest {
    @get:Rule
    val folder = TemporaryFolder()

    @get:Rule
    val mainThread = MainThreadRule()

    @Test
    fun aPhoneWithoutWorkspacesSaysSo() = runBlocking {
        val home = HomeViewModel(testCore(folder.root))
        val state = withTimeout(5_000) { home.state.first { it !is HomeState.Loading } }
        assertEquals(HomeState.NoWorkspace(), state)
    }

    @Test
    fun aCreateThatFailsKeepsTheFormAndSaysWhy() = runBlocking {
        val home = HomeViewModel(testCore(folder.root))
        withTimeout(5_000) { home.state.first { it is HomeState.NoWorkspace } }

        // The core refuses a name with an invisible character in it.
        home.createWorkspace("Acme", memberName = "A\u200Bda", unusedBucket())

        val state = withTimeout(5_000) {
            home.state.first { it is HomeState.NoWorkspace && it.createError != null }
        } as HomeState.NoWorkspace
        assertEquals(false, state.isCreating)
    }

    @Test
    fun aWorkspaceShowsItsChannels() = runBlocking {
        val core = testCore(folder.root)
        core.createAcme()
        val home = HomeViewModel(core)

        val ready = withTimeout(5_000) { home.state.first { it is HomeState.Ready } } as HomeState.Ready
        assertEquals("Acme", ready.workspaceName)
        assertEquals(listOf("Ada"), ready.members.map { it.name })
        assertEquals(listOf("general"), ready.channels.map { it.name })
    }

    @Test
    fun aReplacedWorkspaceNoLongerSteersHome() = runBlocking {
        val core = testCore(folder.root)
        val home = HomeViewModel(core)
        withTimeout(5_000) { home.state.first { it is HomeState.NoWorkspace } }
        home.createWorkspace("Acme", "Ada", unusedBucket())
        val acme = withTimeout(5_000) { home.state.first { it is HomeState.Ready } as HomeState.Ready }

        home.createWorkspace("Beta", "Ada", unusedBucket())
        withTimeout(5_000) { home.state.first { it is HomeState.Ready && it.workspaceName == "Beta" } }
        core.app.workspace(acme.workspaceId)?.use { it.sendMessage(acme.channels.first().id, "still there?", null) }
        delay(500)

        assertEquals("Beta", (home.state.value as HomeState.Ready).workspaceName)
    }
}
