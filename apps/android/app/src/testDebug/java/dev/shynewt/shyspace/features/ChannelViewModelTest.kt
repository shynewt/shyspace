package dev.shynewt.shyspace.features

import dev.shynewt.shyspace.MainThreadRule
import dev.shynewt.shyspace.createAcme
import dev.shynewt.shyspace.features.channel.ChannelViewModel
import dev.shynewt.shyspace.testCore
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder

class ChannelViewModelTest {
    @get:Rule
    val folder = TemporaryFolder()

    @get:Rule
    val mainThread = MainThreadRule()

    @Test
    fun aSentMessageShowsUpNewestFirst() = runBlocking {
        val core = testCore(folder.root)
        val acme = core.createAcme()
        val general = acme.channels().single().id
        acme.sendMessage(general, "first", null)
        val channel = ChannelViewModel(core, acme.workspaceId(), general)
        withTimeout(5_000) { channel.state.first { it.messages.isNotEmpty() } }

        assertTrue(channel.send("  second  "))

        val state = withTimeout(5_000) { channel.state.first { it.messages.size == 2 } }
        assertEquals(listOf("second", "first"), state.messages.map { it.text })
    }

    @Test
    fun blankTextIsNotSent() = runBlocking {
        val core = testCore(folder.root)
        val acme = core.createAcme()
        val channel = ChannelViewModel(core, acme.workspaceId(), acme.channels().single().id)
        withTimeout(5_000) { channel.state.first { !it.isLoading } }

        assertFalse(channel.send("   "))
        assertTrue(channel.state.value.messages.isEmpty())
    }
}
