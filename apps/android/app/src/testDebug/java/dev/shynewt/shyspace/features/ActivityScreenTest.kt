package dev.shynewt.shyspace.features

import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.assert
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.v2.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import dev.shynewt.shyspace.MainTab
import dev.shynewt.shyspace.MainTabBar
import dev.shynewt.shyspace.features.activity.ActivityScreen
import dev.shynewt.shyspace.features.activity.ActivityState
import dev.shynewt.shyspace.sampleChannel
import dev.shynewt.shyspace.sampleMessage
import dev.shynewt.shyspace.ui.theme.ShyTheme
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import uniffi.shy_core.InboxItem
import uniffi.shy_core.InboxReason

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class ActivityScreenTest {
    @get:Rule
    val compose = createComposeRule()

    /** What the screen asked for, in order. */
    private val asked = mutableListOf<String>()

    private val mention = InboxItem(
        message = sampleMessage("m1", "Grace", "@Ada can you check the **build**?").copy(channel = "general"),
        channelName = "general",
        threadRoot = null,
        reason = InboxReason.MENTION,
    )
    private val reply = InboxItem(
        message = sampleMessage("r1", "Linus", "noon works").copy(channel = "design"),
        channelName = "design",
        threadRoot = "root1",
        reason = InboxReason.THREAD_REPLY,
    )

    private fun show(state: ActivityState) {
        compose.setContent {
            ShyTheme(isDark = false) {
                ActivityScreen(
                    state = state,
                    channels = listOf(sampleChannel("general"), sampleChannel("design")),
                    members = emptyList(),
                    onOpenItem = { item, title -> asked += "open ${item.message.id} ${item.threadRoot} $title" },
                    onOpenChannel = { channel, title -> asked += "channel ${channel.id} $title" },
                    onMarkRead = { asked += "read $it" },
                    bottomBar = {},
                )
            }
        }
    }

    @Test
    fun mentionsAndRepliesSayWhoAndWhereAndOpenWhereTheyAre() {
        show(
            ActivityState(
                attention = listOf(mention, reply),
                previews = mapOf("m1" to "@Ada can you check the build?"),
                isLoading = false,
            ),
        )

        compose.onNodeWithText("Grace mentioned you in #general").assertExists()
        compose.onNodeWithText("@Ada can you check the build?").assertExists()
        compose.onNodeWithText("Linus replied in a thread in #design").performClick()
        compose.onNodeWithText("Grace mentioned you in #general").performClick()

        assertEquals(listOf("open r1 root1 #design", "open m1 null #general"), asked)
    }

    @Test
    fun anUnreadChannelOpensOrIsMarkedRead() {
        show(ActivityState(unreadChannels = listOf(sampleChannel("random", unread = 3u, mentions = 1u)), isLoading = false))

        compose.onNodeWithContentDescription("1 mention", substring = true).assertExists()
        compose.onNodeWithContentDescription("Mark #random read").performClick()
        compose.onNodeWithText("#random").performClick()

        assertEquals(listOf("read random", "channel random #random"), asked)
    }

    @Test
    fun withNothingWaitingItSaysYouAreCaughtUp() {
        show(ActivityState(isLoading = false))

        compose.onNodeWithText("You're all caught up").assertExists()
    }

    @Test
    fun theActivityTabSaysHowManyAreWaiting() {
        compose.setContent {
            ShyTheme(isDark = false) { MainTabBar(MainTab.HOME, activityCount = 2, onSelect = {}) }
        }

        compose.onNode(SemanticsMatcher.expectValue(SemanticsProperties.StateDescription, "2 waiting") and hasText("Activity"))
            .assertExists()
    }

    @Test
    fun anErrorShowsAboveTheListWhichStays() {
        show(ActivityState(unreadChannels = listOf(sampleChannel("random", unread = 3u)), isLoading = false, error = "couldn't mark #random read"))

        compose.onNodeWithText("couldn't mark #random read").assertExists()
        compose.onNodeWithText("#random").assertExists()
    }

    @Test
    fun withNoWorkspaceItSaysWhyInsteadOfLoading() {
        show(ActivityState(isLoading = false, error = "No workspaces on this phone yet."))

        compose.onNodeWithText("No workspaces on this phone yet.").assertExists()
        compose.onNodeWithText("You're all caught up").assertDoesNotExist()
    }

    @Test
    fun itsRowsAreButtons() {
        show(ActivityState(attention = listOf(mention), unreadChannels = listOf(sampleChannel("random", unread = 3u)), isLoading = false))

        val isButton = SemanticsMatcher.expectValue(SemanticsProperties.Role, Role.Button)
        compose.onNodeWithText("Grace mentioned you in #general").assert(isButton)
        compose.onNodeWithText("#random").assert(isButton)
    }
}
