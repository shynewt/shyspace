package dev.shynewt.shyspace.features

import dev.shynewt.shyspace.MainThreadRule
import dev.shynewt.shyspace.createAcme
import dev.shynewt.shyspace.features.home.HomeState
import dev.shynewt.shyspace.features.home.HomeViewModel
import dev.shynewt.shyspace.testCore
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
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
        assertEquals(HomeState.NoWorkspace, state)
    }

    @Test
    fun aWorkspaceShowsItsChannels() = runBlocking {
        val core = testCore(folder.root)
        core.createAcme()
        val home = HomeViewModel(core)

        val ready = withTimeout(5_000) { home.state.first { it is HomeState.Ready } } as HomeState.Ready
        assertEquals("Acme", ready.workspaceName)
        assertEquals(listOf("general"), ready.channels.map { it.name })
    }
}
