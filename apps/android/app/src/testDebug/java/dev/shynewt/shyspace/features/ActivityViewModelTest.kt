package dev.shynewt.shyspace.features

import dev.shynewt.shyspace.MainThreadRule
import dev.shynewt.shyspace.createAcme
import dev.shynewt.shyspace.features.activity.ActivityViewModel
import dev.shynewt.shyspace.testCore
import org.junit.Assert.assertFalse
import dev.shynewt.shyspace.unusedBucket
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder

class ActivityViewModelTest {
    @get:Rule
    val folder = TemporaryFolder()

    @get:Rule
    val mainThread = MainThreadRule()

    @Test
    fun followingAWorkspaceLoadsItsActivity() = runBlocking {
        val core = testCore(folder.root)
        val acme = core.createAcme()
        acme.sendMessage(acme.channels().single().id, "hello", null)
        val activity = mainThread.viewModel { ActivityViewModel(core) }

        activity.follow(acme.workspaceId())

        val loaded = withTimeout(5_000) { activity.state.first { !it.isLoading } }
        assertEquals(acme.workspaceId(), loaded.workspaceId)
        assertNull(loaded.error)
        assertTrue("your own messages are never waiting for you", loaded.attention.isEmpty() && loaded.unreadChannels.isEmpty())
    }

    @Test
    fun aWorkspaceThatIsNotOnThePhoneSaysSo() = runBlocking {
        val activity = mainThread.viewModel { ActivityViewModel(testCore(folder.root)) }

        activity.follow("missing")

        val shown = withTimeout(5_000) { activity.state.first { !it.isLoading } }
        assertEquals("This workspace isn't on this phone.", shown.error)
    }

    @Test
    fun markingAChannelReadGoesThroughTheCore() = runBlocking {
        val core = testCore(folder.root)
        val acme = core.createAcme()
        val general = acme.channels().single().id
        acme.sendMessage(general, "hello", null)
        val activity = mainThread.viewModel { ActivityViewModel(core) }
        activity.follow(acme.workspaceId())
        withTimeout(5_000) { activity.state.first { !it.isLoading } }

        activity.markChannelRead(general)
        activity.markChannelRead("no-such-channel")

        val failed = withTimeout(5_000) { activity.state.first { it.error != null } }
        assertEquals(acme.workspaceId(), failed.workspaceId)
        assertFalse(failed.isLoading)
    }

    @Test
    fun followingAnotherWorkspaceShowsThatOne() = runBlocking {
        val core = testCore(folder.root)
        val acme = core.createAcme()
        val second = core.app.createWorkspace("Beta", "Ada", unusedBucket())
        val activity = mainThread.viewModel { ActivityViewModel(core) }
        activity.follow(acme.workspaceId())
        withTimeout(5_000) { activity.state.first { !it.isLoading } }

        activity.follow(second.workspaceId())

        val shown = withTimeout(5_000) { activity.state.first { !it.isLoading && it.workspaceId == second.workspaceId() } }
        assertNull(shown.error)
    }

    @Test
    fun withNoWorkspaceItSaysWhyAndStopsLoading() {
        val activity = mainThread.viewModel { ActivityViewModel(testCore(folder.root)) }

        activity.showUnavailable("No workspaces on this phone yet.")

        assertEquals("No workspaces on this phone yet.", activity.state.value.error)
        assertFalse(activity.state.value.isLoading)
    }
}
