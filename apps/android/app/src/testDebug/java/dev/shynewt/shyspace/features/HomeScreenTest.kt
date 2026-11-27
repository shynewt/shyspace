package dev.shynewt.shyspace.features

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
    fun onlyChannelsYouAreInShowAndDirectMessagesWaitForNames() {
        showHome(
            sampleChannel("general"),
            sampleChannel("archive").copy(isMember = false),
            sampleChannel("dm-ada", kind = ChannelKind.DM),
        )

        compose.onNodeWithText("general").assertExists()
        compose.onNodeWithText("archive").assertDoesNotExist()
        compose.onNodeWithText("dm-ada").assertDoesNotExist()
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
}

private fun androidx.compose.ui.test.SemanticsNodeInteraction.assertStateDescriptionEquals(expected: String) =
    assert(SemanticsMatcher.expectValue(SemanticsProperties.StateDescription, expected))
