package dev.shynewt.shyspace

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import uniffi.shy_core.Change

class ShyCoreChangesTest {
    @get:Rule
    val folder = TemporaryFolder()

    @Test
    fun changesStartWithASyncOnceListeningSoALoadOnItMissesNothing() = runBlocking {
        val core = testCore(folder.root)
        val acme = core.createAcme()
        val general = acme.channels().single().id
        val received = Channel<Change>(Channel.UNLIMITED)
        val listening = launch(Dispatchers.Default) { core.changes(acme).collect { received.send(it) } }

        assertEquals(Change.Sync, withTimeout(5_000) { received.receive() })
        acme.sendMessage(general, "right after the first load", null)
        val next = withTimeout(5_000) {
            var change = received.receive()
            while (change !is Change.Messages) change = received.receive()
            change
        }

        assertEquals(general, next.channel)
        listening.cancel()
    }
}
