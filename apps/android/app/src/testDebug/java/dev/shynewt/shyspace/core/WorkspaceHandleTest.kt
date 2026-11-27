package dev.shynewt.shyspace.core

import dev.shynewt.shyspace.createAcme
import dev.shynewt.shyspace.testCore
import kotlinx.coroutines.flow.flowOf
import kotlinx.coroutines.flow.toList
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotSame
import org.junit.Assert.assertThrows
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import uniffi.shy_core.Change

class WorkspaceHandleTest {
    @get:Rule
    val folder = TemporaryFolder()

    @Test
    fun whenTheReadingWorkspaceClosesTheHandleSwapsForTheJoinedOneAndCarriesOn() = runBlocking {
        val core = testCore(folder.root)
        val acme = core.createAcme()
        val general = acme.channels().single().id
        var runs = 0
        // The core's changes as a linked phone sees them: reading, then the joined workspace in its place.
        val handle = WorkspaceHandle(core, acme.workspaceId(), onFailure = { error(it) }) {
            runs++
            if (runs == 1) flowOf(Change.Sync, Change.ReadingClosed, Change.Channels) else flowOf(Change.Sync)
        }
        val reading = checkNotNull(handle.get())

        val seen = withTimeout(5_000) { handle.changes().toList() }

        assertEquals(listOf<Change>(Change.Sync, Change.Sync), seen)
        assertThrows(IllegalStateException::class.java) { runBlocking { reading.channels() } }
        val joined = checkNotNull(handle.get())
        assertNotSame(reading, joined)
        assertEquals(general, joined.channels().single().id)
        handle.close()
    }
}
