package dev.shynewt.shyspace.features

import dev.shynewt.shyspace.MainThreadRule
import dev.shynewt.shyspace.MemoryKeyStore
import dev.shynewt.shyspace.createAcme
import dev.shynewt.shyspace.features.channel.ChannelViewModel
import dev.shynewt.shyspace.features.channel.ThreadViewModel
import dev.shynewt.shyspace.testCore
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder

/** Android can restore the app straight onto a deep screen, before anything opened the workspaces. */
class RestoreTest {
    @get:Rule
    val folder = TemporaryFolder()

    @get:Rule
    val mainThread = MainThreadRule()

    @Test
    fun screensRestoredBeforeAnythingOpenedTheWorkspaceStillLoad() = runBlocking {
        val keys = MemoryKeyStore()
        val before = testCore(folder.root, keys)
        val acme = before.createAcme()
        val workspaceId = acme.workspaceId()
        val general = acme.channels().single().id
        val root = acme.sendMessage(general, "lunch?", null)
        acme.sendMessage(general, "yes", root)
        // As the process ends: nothing of the first app is left holding the workspace's folder.
        before.app.closeAll()
        acme.close()
        before.app.close()

        val restored = testCore(folder.root, keys)
        val channel = mainThread.viewModel { ChannelViewModel(restored, workspaceId, general) }
        val thread = mainThread.viewModel { ThreadViewModel(restored, workspaceId, general, root) }

        val channelShown = withTimeout(5_000) { channel.state.first { it.error != null || !it.isLoading } }
        val threadShown = withTimeout(5_000) { thread.state.first { it.error != null || !it.isLoading } }
        assertNull(channelShown.error)
        assertNull(threadShown.error)
        assertEquals(listOf("lunch?"), channelShown.messages.map { it.text })
        assertEquals(listOf("yes"), threadShown.replies.map { it.text })
    }

    @Test
    fun aRestoredScreenWhoseWorkspaceCannotOpenPointsToHomeWithoutThePath() = runBlocking {
        val before = testCore(folder.root)
        val acme = before.createAcme()
        val workspaceId = acme.workspaceId()
        val general = acme.channels().single().id
        before.app.closeAll()
        acme.close()
        before.app.close()

        val withoutItsKey = testCore(folder.root, MemoryKeyStore())
        val channel = mainThread.viewModel { ChannelViewModel(withoutItsKey, workspaceId, general) }

        val shown = withTimeout(5_000) { channel.state.first { it.error != null } }
        assertEquals("This workspace couldn't open. Home shows why.", shown.error)
    }
}
