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

    @Test
    fun messagesShowAsPlainTextWithoutMarkdownMarks() = runBlocking {
        val core = testCore(folder.root)
        val acme = core.createAcme()
        val general = acme.channels().single().id
        val id = acme.sendMessage(general, "**ship** it `now`", null)
        val channel = ChannelViewModel(core, acme.workspaceId(), general)

        val state = withTimeout(5_000) { channel.state.first { it.messages.isNotEmpty() } }

        assertEquals("ship it now", state.plainTexts[id])
    }

    @Test
    fun seeingTheNewestMessageMarksTheChannelReadUpToIt() = runBlocking {
        val core = testCore(folder.root)
        val acme = core.createAcme()
        val general = acme.channels().single().id
        acme.sendMessage(general, "first", null)
        val newest = acme.sendMessage(general, "second", null)
        val channel = ChannelViewModel(core, acme.workspaceId(), general)
        withTimeout(5_000) { channel.state.first { it.messages.size == 2 } }

        channel.markSeen(newest)

        val read = withTimeout(5_000) {
            var lastRead = acme.channels().single().lastRead
            while (lastRead == null) lastRead = acme.channels().single().lastRead
            lastRead
        }
        val newestMessage = channel.state.value.messages.first()
        assertEquals(newestMessage.lamport to newestMessage.device, read.lamport to read.device)
    }

    @Test
    fun aMessageCanBeReactedToEditedAndDeleted() = runBlocking {
        val core = testCore(folder.root)
        val acme = core.createAcme()
        val general = acme.channels().single().id
        val id = acme.sendMessage(general, "hello", null)
        val channel = ChannelViewModel(core, acme.workspaceId(), general)
        withTimeout(5_000) { channel.state.first { it.messages.isNotEmpty() } }

        channel.react(id, "👍", active = true)
        withTimeout(5_000) { channel.state.first { it.messages.single().reactions.any { r -> r.emoji == "👍" && r.mine } } }

        assertTrue(channel.edit(id, " hello, again "))
        val edited = withTimeout(5_000) { channel.state.first { it.messages.single().isEdited } }
        assertEquals("hello, again", edited.messages.single().text)

        channel.delete(id)
        val deleted = withTimeout(5_000) { channel.state.first { it.messages.isEmpty() } }
        assertEquals(null, deleted.error)
    }
}
