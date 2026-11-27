package dev.shynewt.shyspace.features

import dev.shynewt.shyspace.MainThreadRule
import dev.shynewt.shyspace.createAcme
import dev.shynewt.shyspace.features.channel.ThreadViewModel
import dev.shynewt.shyspace.testCore
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder

class ThreadViewModelTest {
    @get:Rule
    val folder = TemporaryFolder()

    @get:Rule
    val mainThread = MainThreadRule()

    @Test
    fun aThreadLoadsAndTakesReplies() = runBlocking {
        val core = testCore(folder.root)
        val acme = core.createAcme()
        val general = acme.channels().single().id
        val root = acme.sendMessage(general, "lunch?", null)
        acme.sendMessage(general, "yes", root)
        val thread = ThreadViewModel(core, acme.workspaceId(), general, root)
        val loaded = withTimeout(5_000) { thread.state.first { !it.isLoading } }
        assertEquals("lunch?", loaded.root?.text)
        assertEquals(listOf("yes"), loaded.replies.map { it.text })

        assertTrue(thread.reply("  noon works  "))

        val replied = withTimeout(5_000) { thread.state.first { it.replies.size == 2 } }
        assertEquals(listOf("noon works", "yes"), replied.replies.map { it.text })
    }

    @Test
    fun aThreadWhoseFirstMessageIsDeletedIsGone() = runBlocking {
        val core = testCore(folder.root)
        val acme = core.createAcme()
        val general = acme.channels().single().id
        val root = acme.sendMessage(general, "lunch?", null)
        val thread = ThreadViewModel(core, acme.workspaceId(), general, root)
        withTimeout(5_000) { thread.state.first { !it.isLoading } }

        acme.deleteMessage(root)

        val gone = withTimeout(5_000) { thread.state.first { it.isGone } }
        assertNull(gone.error)
    }
}
