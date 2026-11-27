package dev.shynewt.shyspace.features

import kotlinx.coroutines.launch
import kotlinx.coroutines.Dispatchers
import dev.shynewt.shyspace.MainThreadRule
import dev.shynewt.shyspace.createAcme
import dev.shynewt.shyspace.features.channel.ChannelState
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
        val channel = mainThread.viewModel { ChannelViewModel(core, acme.workspaceId(), general) }
        withTimeout(5_000) { channel.state.first { it.messages.isNotEmpty() } }

        assertTrue(channel.send("  second  "))

        val state = withTimeout(5_000) { channel.state.first { it.messages.size == 2 } }
        assertEquals(listOf("second", "first"), state.messages.map { it.text })
    }

    @Test
    fun blankTextIsNotSent() = runBlocking {
        val core = testCore(folder.root)
        val acme = core.createAcme()
        val general = acme.channels().single().id
        val channel = mainThread.viewModel { ChannelViewModel(core, acme.workspaceId(), general) }
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
        val channel = mainThread.viewModel { ChannelViewModel(core, acme.workspaceId(), general) }

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
        val channel = mainThread.viewModel { ChannelViewModel(core, acme.workspaceId(), general) }
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
        val channel = mainThread.viewModel { ChannelViewModel(core, acme.workspaceId(), general) }
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

    @Test
    fun olderMessagesLoadAPageAtATimeUntilNoneAreLeft() = runBlocking {
        val core = testCore(folder.root)
        val acme = core.createAcme()
        val general = acme.channels().single().id
        repeat(120) { acme.sendMessage(general, "message $it", null) }
        val channel = mainThread.viewModel { ChannelViewModel(core, acme.workspaceId(), general) }
        withTimeout(5_000) { channel.state.first { it.messages.size == 50 } }
        assertTrue(channel.state.value.hasOlder)

        channel.loadOlder()
        withTimeout(5_000) { channel.state.first { it.messages.size == 100 } }
        channel.loadOlder()
        val all = withTimeout(5_000) { channel.state.first { it.messages.size == 120 && !it.hasOlder } }

        assertEquals((119 downTo 0).map { "message $it" }, all.messages.map { it.text })
    }

    @Test
    fun messagesArrivingWhileOlderOnesLoadLeaveNoGapAndDropNoPage() = runBlocking {
        val core = testCore(folder.root)
        val acme = core.createAcme()
        val general = acme.channels().single().id
        val sent = (0 until 120).map { "message $it" }.toMutableList()
        sent.forEach { acme.sendMessage(general, it, null) }

        repeat(8) { round ->
            val channel = mainThread.viewModel { ChannelViewModel(core, acme.workspaceId(), general) }
            withTimeout(5_000) { channel.state.first { it.messages.size == 50 } }

            val arriving = launch(Dispatchers.Default) {
                repeat(3) { index ->
                    val text = "new $round.$index"
                    acme.sendMessage(general, text, null)
                    synchronized(sent) { sent += text }
                }
            }
            channel.loadOlder()
            arriving.join()

            val newest = synchronized(sent) { sent.last() }
            val settled = withTimeout(5_000) {
                channel.state.first { !it.isLoadingOlder && it.messages.size == 100 && it.messages.first().text == newest }
            }
            val expected = synchronized(sent) { sent.takeLast(100).reversed() }
            assertEquals("round $round", expected, settled.messages.map { it.text })
        }
    }

    @Test
    fun atTheChannelsStartANewMessageDoesNotOfferOlderOnesAgain() = runBlocking {
        val core = testCore(folder.root)
        val acme = core.createAcme()
        val general = acme.channels().single().id
        repeat(100) { acme.sendMessage(general, "message $it", null) }
        val channel = mainThread.viewModel { ChannelViewModel(core, acme.workspaceId(), general) }
        withTimeout(5_000) { channel.state.first { it.messages.size == 50 } }
        channel.loadOlder()
        withTimeout(5_000) { channel.state.first { it.messages.size == 100 && !it.isLoadingOlder } }
        channel.loadOlder()
        withTimeout(5_000) { channel.state.first { !it.hasOlder && !it.isLoadingOlder } }

        acme.sendMessage(general, "one more", null)

        val after = withTimeout(5_000) { channel.state.first { it.messages.first().text == "one more" } }
        assertFalse(after.hasOlder)
    }

    @Test
    fun aChannelThatGrowsWhileOpenStillReachesItsOlderMessages() = runBlocking {
        val core = testCore(folder.root)
        val acme = core.createAcme()
        val general = acme.channels().single().id
        repeat(10) { acme.sendMessage(general, "early $it", null) }
        val channel = mainThread.viewModel { ChannelViewModel(core, acme.workspaceId(), general) }
        withTimeout(5_000) { channel.state.first { it.messages.size == 10 } }

        repeat(100) { acme.sendMessage(general, "later $it", null) }
        withTimeout(5_000) { channel.state.first { it.messages.size == 50 && it.hasOlder } }
        channel.loadOlder()
        withTimeout(5_000) { channel.state.first { it.messages.size == 100 && !it.isLoadingOlder } }
        channel.loadOlder()

        val all = withTimeout(5_000) { channel.state.first { it.messages.size == 110 && !it.isLoadingOlder } }
        assertEquals("early 0", all.messages.last().text)
        assertFalse(all.hasOlder)
    }

    @Test
    fun aChangeDuringAnOlderLoadNeverEndsItsSpinnerEarly() = runBlocking {
        val core = testCore(folder.root)
        val acme = core.createAcme()
        val general = acme.channels().single().id
        repeat(120) { acme.sendMessage(general, "message $it", null) }
        val channel = mainThread.viewModel { ChannelViewModel(core, acme.workspaceId(), general) }
        withTimeout(5_000) { channel.state.first { it.messages.size == 50 } }
        val seen = mutableListOf<ChannelState>()
        val watching = launch { channel.state.collect { synchronized(seen) { seen += it } } }

        // A reload already running when older ones are asked for: it must not end their spinner.
        acme.sendMessage(general, "before", null)
        channel.loadOlder()
        acme.sendMessage(general, "during", null)
        withTimeout(5_000) { channel.state.first { it.messages.size >= 100 && !it.isLoadingOlder && it.messages.first().text == "during" } }
        watching.cancel()

        val endedEarly = synchronized(seen) { seen.filter { !it.isLoadingOlder && it.messages.size < 100 && it !== seen.first() } }
        assertEquals("states that dropped the spinner before the older page came", emptyList<ChannelState>(), endedEarly)
    }
}
