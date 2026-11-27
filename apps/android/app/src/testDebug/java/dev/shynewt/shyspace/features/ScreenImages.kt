package dev.shynewt.shyspace.features

import dev.shynewt.shyspace.ui.Logo
import dev.shynewt.shyspace.features.onboarding.WelcomeScreen
import dev.shynewt.shyspace.features.onboarding.LinkStep
import dev.shynewt.shyspace.features.onboarding.LinkScreen
import androidx.compose.runtime.Composable
import com.github.takahirom.roborazzi.captureRoboImage
import dev.shynewt.shyspace.features.channel.ChannelScreen
import dev.shynewt.shyspace.features.activity.ActivityScreen
import dev.shynewt.shyspace.features.activity.ActivityState
import dev.shynewt.shyspace.features.channel.ChannelState
import uniffi.shy_core.InboxItem
import uniffi.shy_core.InboxReason
import dev.shynewt.shyspace.features.channel.ThreadScreen
import dev.shynewt.shyspace.features.channel.ThreadState
import dev.shynewt.shyspace.features.home.HomeScreen
import dev.shynewt.shyspace.features.home.HomeState
import dev.shynewt.shyspace.features.home.DevelopmentWorkspaceTools
import dev.shynewt.shyspace.MainTab
import dev.shynewt.shyspace.MainTabBar
import dev.shynewt.shyspace.features.settings.YouScreen
import dev.shynewt.shyspace.ui.Avatar
import dev.shynewt.shyspace.ui.Badge
import dev.shynewt.shyspace.ui.BadgeTone
import dev.shynewt.shyspace.ui.CountBadge
import dev.shynewt.shyspace.ui.DayDivider
import dev.shynewt.shyspace.ui.SectionLabel
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.padding
import androidx.compose.ui.Modifier
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.ui.unit.dp
import dev.shynewt.shyspace.ui.theme.LocalShyColors
import uniffi.shy_core.NotifyLevel
import dev.shynewt.shyspace.sampleChannel
import dev.shynewt.shyspace.sampleMember
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
    private val MENTION_AND_LINK = "[@Linus](shy://member/linus) the jump is in the docs too: [the docs](https://example.com)"
    // This morning at 8, so the day reads "Today" as it would in use.
    private val start = java.time.LocalDate.now().atTime(8, 0).atZone(java.time.ZoneId.systemDefault()).toInstant().toEpochMilli()

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
                    sampleChannel("watercooler", unread = 9u).copy(notify = NotifyLevel.NOTHING),
                    sampleChannel("notes", kind = ChannelKind.NOTES),
                    sampleChannel("dm-grace", unread = 1u, kind = ChannelKind.DM, dmMembers = listOf("grace")),
                    sampleChannel("dm-linus", kind = ChannelKind.DM, dmMembers = listOf("linus")),
                    sampleChannel("dm-helper", kind = ChannelKind.DM, dmMembers = listOf("helper")),
                ),
                openFailures = emptyList(),
                members = listOf(sampleMember("grace", "Grace Hopper"), sampleMember("linus", "Linus"), sampleMember("helper", "Helper", isBot = true)),
            ),
            onOpenChannel = { _, _ -> },
            onRetry = {},
            bottomBar = { MainTabBar(MainTab.HOME, activityCount = 2, onSelect = {}) },
            noWorkspaceContent = {},
        )
    }

    @Test
    fun activity() = capture("activity") {
        ActivityScreen(
            state = ActivityState(
                attention = listOf(
                    InboxItem(
                        sampleMessage("9", "Grace", "linking", start + 31 * minute).copy(channel = "general"),
                        channelName = "general",
                        threadRoot = null,
                        reason = InboxReason.MENTION,
                    ),
                    InboxItem(
                        sampleMessage("8", "Linus", "noon", start + 9 * minute).copy(channel = "design"),
                        channelName = "design",
                        threadRoot = "3",
                        reason = InboxReason.THREAD_REPLY,
                    ),
                ),
                previews = mapOf(
                    "9" to "@Ada can you check how the check code looks on your phone before the release?",
                    "8" to "Noon works for me.",
                ),
                unreadChannels = listOf(sampleChannel("random", unread = 3u), sampleChannel("design", unread = 4u, mentions = 1u)),
                isLoading = false,
            ),
            channels = listOf(sampleChannel("general"), sampleChannel("design"), sampleChannel("random")),
            members = emptyList(),
            onOpenItem = { _, _ -> },
            onOpenChannel = { _, _ -> },
            onMarkRead = {},
            bottomBar = { MainTabBar(MainTab.ACTIVITY, activityCount = 2, onSelect = {}) },
        )
    }

    @Test
    fun activityCaughtUp() = capture("activity-caught-up") {
        ActivityScreen(
            state = ActivityState(isLoading = false),
            channels = emptyList(),
            members = emptyList(),
            onOpenItem = { _, _ -> },
            onOpenChannel = { _, _ -> },
            onMarkRead = {},
            bottomBar = { MainTabBar(MainTab.ACTIVITY, activityCount = 0, onSelect = {}) },
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
            sampleMessage("6", "Grace", MENTION_AND_LINK, start + 20 * minute),
            sampleMessage("7", "Helper", "I summarised the thread above.", start + 21 * minute).copy(authorIsBot = true),
        )
        ChannelScreen(
            title = "#general",
            state = ChannelState(
                messages = messages.reversed(),
                plainTexts = mapOf("6" to "@Linus the jump is in the docs too: the docs"),
                isLoading = false,
            ),
            onBack = {},
            onSend = { true },
            onSeen = {},
            onEdit = { _, _ -> true },
            onReact = { _, _, _ -> },
            onDelete = {},
            onLoadOlder = {},
            onOpenThread = {},
        )
    }

    @Test
    fun thread() = capture("thread") {
        ThreadScreen(
            channelTitle = "#general",
            state = ThreadState(
                root = sampleMessage("3", "Grace", "On it. Mine is a Pixel, I'll report back here.", start + 5 * minute, replyCount = 2u),
                replies = listOf(
                    sampleMessage("9", "Grace", "Linking worked first time. The check code matched.", start + 31 * minute),
                    sampleMessage("8", "Ada", "Tell me how the check code looks on yours.", start + 9 * minute),
                ),
                isLoading = false,
            ),
            onBack = {}, onReply = { true }, onSeen = {}, onEdit = { _, _ -> true }, onReact = { _, _, _ -> }, onDelete = {},
        )
    }

    @Test
    fun emptyChannel() = capture("channel-empty") {
        ChannelScreen(title = "#releases", state = ChannelState(isLoading = false), onBack = {}, onSend = { true }, onSeen = {}, onEdit = { _, _ -> true }, onReact = { _, _, _ -> }, onDelete = {}, onLoadOlder = {}, onOpenThread = {})
    }

    @Test
    fun you() = capture("you") {
        YouScreen(hidesScreenContent = true, onHidesScreenContentChange = {}, saveFailed = false, bottomBar = { MainTabBar(MainTab.YOU, activityCount = 0, onSelect = {}) })
    }

    /** The shared pieces in ui/, each in its variants. */
    @Test
    fun gallery() = capture("gallery") {
        Column(
            verticalArrangement = Arrangement.spacedBy(16.dp),
            modifier = Modifier.fillMaxSize().background(LocalShyColors.current.palette.surface).padding(16.dp),
        ) {
            SectionLabel("Avatar")
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                listOf("Ada Lovelace", "Grace Hopper", "Linus", "Margaret Hamilton", "Alan Kay", "Barbara Liskov")
                    .forEach { Avatar(id = it.lowercase(), name = it) }
                Avatar(id = "helper", name = "Helper", isBot = true)
            }
            SectionLabel("Logo")
            Logo(size = 48.dp)
            SectionLabel("Badge")
            Badge("Agent")
            SectionLabel("CountBadge")
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                CountBadge("3", BadgeTone.ACCENT, "3 new")
                CountBadge("@2", BadgeTone.DANGER, "2 mentions")
                CountBadge("1", BadgeTone.QUIET, "1 draft")
            }
            SectionLabel("DayDivider")
            DayDivider("Today")
            SectionLabel("Material controls in our colours")
            Row(horizontalArrangement = Arrangement.spacedBy(16.dp)) {
                Switch(checked = false, onCheckedChange = {})
                Switch(checked = true, onCheckedChange = {})
            }
            OutlinedTextField(value = "", onValueChange = {}, label = { Text("Bucket address") })
        }
    }

    @Test
    fun welcome() = capture("welcome") {
        WelcomeScreen(onLinkThisPhone = {}, developerTools = null, createError = null)
    }

    @Test
    fun welcomeBeforeLinking() = capture("welcome-before-linking") {
        WelcomeScreen(
            onLinkThisPhone = null,
            developerTools = { DevelopmentWorkspaceTools(isCreating = false, onCreate = { _, _, _ -> }) },
            createError = null,
        )
    }

    @Test
    fun linkEnterCode() = capture("link-enter-code") {
        LinkScreen(step = LinkStep.EnterCode(hasExpired = true), onCodeChange = {}, onStartAgain = {}, onBack = {})
    }

    @Test
    fun linkChecking() = capture("link-checking") {
        LinkScreen(step = LinkStep.Checking("482917"), onCodeChange = {}, onStartAgain = {}, onBack = {})
    }

    @Test
    fun linkStopped() = capture("link-stopped") {
        LinkScreen(step = LinkStep.Stopped, onCodeChange = {}, onStartAgain = {}, onBack = {})
    }

    @Test
    fun failed() = capture("failed") {
        HomeScreen(
            state = HomeState.Failed("Acme: the workspace's data is in its folder, but its key is missing"),
            onOpenChannel = { _, _ -> },
            onRetry = {},
            bottomBar = {},
            noWorkspaceContent = {},
        )
    }
}
