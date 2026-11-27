package dev.shynewt.shyspace.features.settings

import java.io.File
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancelAndJoin
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.job
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder

class AppSettingsTest {
    @get:Rule
    val folder = TemporaryFolder()

    private val file: File get() = folder.root.resolve("settings.preferences_pb")

    /** Settings over [file] for [block], closed after it, as when the app's process ends. */
    private fun withSettings(block: suspend (AppSettings) -> Unit) = runBlocking {
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
        try {
            block(AppSettings(file, scope))
        } finally {
            scope.coroutineContext.job.cancelAndJoin()
        }
    }

    @Test
    fun screenContentShowsUntilHidden() = withSettings { settings ->
        assertFalse(settings.hidesScreenContent.value)
        settings.setHidesScreenContent(true)
        withTimeout(5_000) { settings.hidesScreenContent.first { it } }
    }

    @Test
    fun theChoiceHoldsFromTheFirstReadAfterARestart() {
        withSettings { settings ->
            settings.setHidesScreenContent(true)
            withTimeout(5_000) { settings.hidesScreenContent.first { it } }
        }
        withSettings { settings -> assertTrue(settings.hidesScreenContent.value) }
    }

    @Test
    fun aDamagedFileHidesTheScreenUntilChosenAgain() {
        file.writeBytes(byteArrayOf(0x13, 0x37, 0x00, 0x7f, 0x2a))
        withSettings { settings ->
            assertTrue(settings.hidesScreenContent.value)
            settings.setHidesScreenContent(false)
            withTimeout(5_000) { settings.hidesScreenContent.first { !it } }
        }
    }

    @Test
    fun aFileThatCantBeReadHidesTheScreen() {
        file.mkdirs()
        withSettings { settings -> assertTrue(settings.hidesScreenContent.value) }
    }

    @Test
    fun aChangeThatCantBeSavedSaysSo() {
        file.mkdirs()
        withSettings { settings ->
            settings.setHidesScreenContent(false)
            withTimeout(5_000) { settings.saveFailed.first { it } }
            assertTrue(settings.hidesScreenContent.value)
        }
    }
}
