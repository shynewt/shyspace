package dev.shynewt.shyspace

import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import uniffi.shy_core.Networking
import uniffi.shy_ffi.ShyApp

class CoreBindingTest {
    @get:Rule
    val folder = TemporaryFolder()

    @Test
    fun aMessageSentThroughTheBindingsComesBackFromTheCore() = runBlocking {
        val keys = MemoryKeyStore()
        val app = ShyApp.withMemoryBucket(folder.root.path, keys, Networking.OFFLINE)

        val acme = app.createWorkspace("Acme", "Ada", unusedBucket())
        val general = acme.channels().first { it.name == "general" }
        acme.sendMessage(general.id, "hello from Kotlin", null)

        val messages = acme.messages(general.id, null, 50u)
        assertEquals(listOf("hello from Kotlin"), messages.map { it.text })
        assertEquals(setOf("device-key-${acme.workspaceId()}"), keys.secrets.keys)
        app.closeAll()
    }
}
