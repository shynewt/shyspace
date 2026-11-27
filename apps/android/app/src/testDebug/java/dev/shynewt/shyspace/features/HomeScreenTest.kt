package dev.shynewt.shyspace.features

import uniffi.shy_core.Member
import uniffi.shy_core.Channel
import androidx.compose.runtime.setValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.getValue
import androidx.compose.ui.test.junit4.v2.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.semantics.SemanticsProperties
import uniffi.shy_core.ChannelKind
import uniffi.shy_core.NotifyLevel
import androidx.compose.ui.test.performClick
import dev.shynewt.shyspace.MainTab
import dev.shynewt.shyspace.MainTabBar
import dev.shynewt.shyspace.sampleChannel
import dev.shynewt.shyspace.sampleMember
import dev.shynewt.shyspace.features.home.HomeScreen
import dev.shynewt.shyspace.features.home.HomeState
import dev.shynewt.shyspace.ui.theme.ShyTheme
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class HomeScreenTest {
    @get:Rule
    val compose = createComposeRule()

    @Test
    fun theYouTabIsOneTapAway() {
        val chosen = mutableListOf<MainTab>()
        compose.setContent {
            ShyTheme(isDark = false) {
                HomeScreen(
                    state = HomeState.Ready("w1", "Acme", channels = emptyList(), openFailures = emptyList()),
                    onOpenChannel = { _, _ -> },
                    onRetry = {},
                    bottomBar = { MainTabBar(MainTab.HOME, onSelect = { chosen += it }) },
                    noWorkspaceContent = {},
                )
            }
        }

        compose.onNodeWithText("You").performClick()

        assertEquals(listOf(MainTab.YOU), chosen)
    }

    @Test
    fun aChannelShowsItsMentionsAndOpensOnATap() {
        val opened = mutableListOf<String>()
        compose.setContent {
            ShyTheme(isDark = false) {
                HomeScreen(
                    state = HomeState.Ready("w1", "Acme", channels = listOf(sampleChannel("design", unread = 3u, mentions = 2u)), openFailures = emptyList()),
                    onOpenChannel = { _, title -> opened += title },
                    onRetry = {},
                    bottomBar = {},
                    noWorkspaceContent = {},
                )
            }
        }

        compose.onNodeWithContentDescription("2 mentions").assertExists()
        compose.onNodeWithText("design").performClick()

        assertEquals(listOf("#design"), opened)
    }

    @Test
    fun workspacesThatWontOpenCanBeTriedAgain() {
        var retries = 0
        compose.setContent {
            ShyTheme(isDark = false) {
                HomeScreen(
                    state = HomeState.Failed("the bucket didn't answer"),
                    onOpenChannel = { _, _ -> },
                    onRetry = { retries++ },
                    bottomBar = {},
                    noWorkspaceContent = {},
                )
            }
        }

        compose.onNodeWithText("Try again").performClick()

        assertEquals(1, retries)
    }

    private fun showHome(vararg channels: uniffi.shy_core.Channel) {
        compose.setContent {
            ShyTheme(isDark = false) {
                HomeScreen(
                    state = HomeState.Ready("w1", "Acme", channels = channels.toList(), openFailures = emptyList()),
                    onOpenChannel = { _, _ -> },
                    onRetry = {},
                    bottomBar = {},
                    noWorkspaceContent = {},
                )
            }
        }
    }

    @Test
    fun onlyChannelsYouAreInShowAndADirectMessageIsNamedForItsPerson() {
        compose.setContent {
            ShyTheme(isDark = false) {
                HomeScreen(
                    state = HomeState.Ready(
                        "w1",
                        "Acme",
                        channels = listOf(
                            sampleChannel("general"),
                            sampleChannel("archive").copy(isMember = false),
                            sampleChannel("dm-1", kind = ChannelKind.DM, dmMembers = listOf("grace-id")),
                            sampleChannel("dm-2", kind = ChannelKind.DM, dmMembers = listOf("f00dfeedcafe")),
                        ),
                        openFailures = emptyList(),
                        members = listOf(sampleMember("grace-id", "Grace")),
                    ),
                    onOpenChannel = { _, _ -> },
                    onRetry = {},
                    bottomBar = {},
                    noWorkspaceContent = {},
                )
            }
        }

        compose.onNodeWithText("general").assertExists()
        compose.onNodeWithText("archive").assertDoesNotExist()
        compose.onNodeWithText("Direct messages").assertExists()
        compose.onNodeWithText("Grace").assertExists()
        compose.onNodeWithText("member f00dfe").assertExists()
        compose.onNodeWithText("dm-1").assertDoesNotExist()
    }

    @Test
    fun aRowSaysWhetherItsUnreadMutedOrPrivate() {
        showHome(
            sampleChannel("design", unread = 2u),
            sampleChannel("watercooler", unread = 5u).copy(notify = NotifyLevel.NOTHING),
            sampleChannel("leads", kind = ChannelKind.PRIVATE),
            sampleChannel("notes", kind = ChannelKind.NOTES),
        )

        compose.onNode(hasText("design")).assertStateDescriptionEquals("Unread")
        compose.onNode(hasText("watercooler")).assertStateDescriptionEquals("Muted")
        compose.onNodeWithContentDescription("Private").assertExists()
        compose.onNodeWithText("Notes").assertExists()
        compose.onNodeWithText("notes").assertDoesNotExist()
    }

    private var shown by mutableStateOf<HomeState>(HomeState.Loading)

    private fun showHomeWith(channels: List<Channel>, members: List<Member>) {
        shown = HomeState.Ready("w1", "Acme", channels = channels, members = members, openFailures = emptyList())
        compose.setContent {
            ShyTheme(isDark = false) {
                HomeScreen(state = shown, onOpenChannel = { _, _ -> }, onRetry = {}, bottomBar = {}, noWorkspaceContent = {})
            }
        }
    }

    @Test
    fun aGroupDirectMessageIsNamedForEveryoneInIt() {
        val group = sampleChannel("dm-group", kind = ChannelKind.DM, dmMembers = listOf("grace", "linus"))
        showHomeWith(listOf(group), listOf(sampleMember("grace", "Grace Hopper"), sampleMember("linus", "Linus")))

        compose.onNodeWithText("Grace Hopper, Linus").assertExists()
    }

    @Test
    fun aDirectMessageWithAnAgentSaysSo() {
        val dm = sampleChannel("dm-helper", kind = ChannelKind.DM, dmMembers = listOf("helper"))
        showHomeWith(listOf(dm), listOf(sampleMember("helper", "Helper", isBot = true)))

        compose.onNode(hasText("Helper")).assertStateDescriptionEquals("Agent")
    }

    @Test
    fun aRenamedPersonsDirectMessageTakesTheNewName() {
        val dm = sampleChannel("dm-grace", kind = ChannelKind.DM, dmMembers = listOf("grace"))
        showHomeWith(listOf(dm), listOf(sampleMember("grace", "Grace")))
        compose.onNodeWithText("Grace").assertExists()

        shown = (shown as HomeState.Ready).copy(members = listOf(sampleMember("grace", "Grace Hopper")))

        compose.onNodeWithText("Grace Hopper").assertExists()
        compose.onNodeWithText("Grace").assertDoesNotExist()
    }

    @Test
    fun aDirectMessageWithNobodyElseStillHasAName() {
        showHomeWith(listOf(sampleChannel("dm-empty", kind = ChannelKind.DM, dmMembers = emptyList())), emptyList())

        compose.onNodeWithText("Direct message").assertExists()
    }

    @Test
    fun withNoDirectMessagesTheirLabelIsHidden() {
        showHomeWith(listOf(sampleChannel("general")), emptyList())

        compose.onNodeWithText("general").assertExists()
        compose.onNodeWithText("Direct messages").assertDoesNotExist()
    }
}

private fun androidx.compose.ui.test.SemanticsNodeInteraction.assertStateDescriptionEquals(expected: String) =
    assert(SemanticsMatcher.expectValue(SemanticsProperties.StateDescription, expected))
