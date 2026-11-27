package dev.shynewt.shyspace.features

import androidx.compose.runtime.Composable
import com.github.takahirom.roborazzi.captureRoboImage
import dev.shynewt.shyspace.features.channel.ChannelScreen
import dev.shynewt.shyspace.features.channel.ChannelState
import dev.shynewt.shyspace.features.home.HomeScreen
import dev.shynewt.shyspace.features.home.HomeState
import dev.shynewt.shyspace.features.home.NoWorkspace
import dev.shynewt.shyspace.features.home.DevelopmentWorkspaceTools
import dev.shynewt.shyspace.features.settings.SettingsScreen
import dev.shynewt.shyspace.sampleChannel
import dev.shynewt.shyspace.sampleMessage
import dev.shynewt.shyspace.ui.theme.ShyTheme
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import uniffi.shy_core.ChannelKind
import uniffi.shy_core.Reaction

/**
 * Every screen as a picture, light and dark, at a large phone's size. Renders only with
 * `make screens` (Roborazzi's record mode); in other test runs it draws nothing.
 */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [35], qualifiers = "w412dp-h892dp-xxhdpi")
class ScreenImages {
    private fun capture(name: String, content: @Composable () -> Unit) {
        for (isDark in listOf(false, true)) {
            val theme = if (isDark) "dark" else "light"
            captureRoboImage("build/outputs/roborazzi/$name-$theme.png") { ShyTheme(isDark = isDark) { content() } }
        }
    }

    private val minute = 60_000L
    private val start = 1_764_230_400_000L // a Thursday morning

    @Test
    fun home() = capture("home") {
        HomeScreen(
            state = HomeState.Ready(
                workspaceId = "w1",
                workspaceName = "Acme",
                channels = listOf(
                    sampleChannel("general", unread = 4u),
                    sampleChannel("design", unread = 2u, mentions = 1u),
                    sampleChannel("releases"),
                    sampleChannel("random"),
                    sampleChannel("leads", kind = ChannelKind.PRIVATE),
                    sampleChannel("notes", kind = ChannelKind.NOTES),
                ),
                openFailures = emptyList(),
            ),
            onOpenChannel = { _, _ -> },
            onOpenSettings = {},
            noWorkspaceContent = {},
        )
    }

    @Test
    fun channel() = capture("channel") {
        val messages = listOf(
            sampleMessage("1", "Ada", "Morning! The new build is on the bucket.", start),
            sampleMessage("2", "Ada", "Can someone try linking a phone?", start + minute),
            sampleMessage("3", "Grace", "On it. Mine is a Pixel, I'll report back here.", start + 5 * minute, replyCount = 3u),
            sampleMessage(
                "4", "Linus", "Works on my tablet too, though the composer jumps when the keyboard opens.", start + 12 * minute,
                reactions = listOf(Reaction(emoji = "👍", count = 2u, mine = true)),
            ),
            sampleMessage("5", "Ada", "Thanks, I'll look at the keyboard jump.", start + 14 * minute),
        )
        ChannelScreen(
            title = "#general",
            state = ChannelState(messages = messages.reversed(), isLoading = false),
            onBack = {},
            onSend = { true },
        )
    }

    @Test
    fun emptyChannel() = capture("channel-empty") {
        ChannelScreen(title = "#releases", state = ChannelState(isLoading = false), onBack = {}, onSend = { true })
    }

    @Test
    fun settings() = capture("settings") {
        SettingsScreen(hidesScreenContent = true, onHidesScreenContentChange = {}, onBack = {})
    }

    @Test
    fun noWorkspace() = capture("no-workspace") {
        NoWorkspace { DevelopmentWorkspaceTools(onCreate = { _, _, _ -> }) }
    }

    @Test
    fun failed() = capture("failed") {
        HomeScreen(
            state = HomeState.Failed("checking the bucket settings: can't write to the bucket: operation timed out"),
            onOpenChannel = { _, _ -> },
            onOpenSettings = {},
            noWorkspaceContent = {},
        )
    }
}
