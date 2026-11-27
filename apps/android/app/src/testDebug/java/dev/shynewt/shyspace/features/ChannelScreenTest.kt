package dev.shynewt.shyspace.features

import androidx.compose.ui.test.assertContentDescriptionContains
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.hasScrollToIndexAction
import androidx.compose.ui.test.performScrollToIndex
import androidx.compose.ui.test.junit4.v2.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import dev.shynewt.shyspace.features.channel.ChannelScreen
import dev.shynewt.shyspace.sampleMessage
import dev.shynewt.shyspace.features.channel.ChannelState
import dev.shynewt.shyspace.ui.theme.ShyTheme
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class ChannelScreenTest {
    @get:Rule
    val compose = createComposeRule()

    @Test
    fun showsMessagesAndSendsWhatIsTyped() {
        val sent = mutableListOf<String>()
        compose.setContent {
            ShyTheme(isDark = false) {
                ChannelScreen(
                    title = "#general",
                    state = ChannelState(messages = listOf(sampleMessage("1", "Ada", "hello")), isLoading = false),
                    onBack = {},
                    onSend = { text -> sent += text; true },
                    onSeen = {},
                )
            }
        }

        compose.onNodeWithText("hello").assertExists()
        compose.onNodeWithText("Ada").assertExists()
        compose.onNodeWithContentDescription("Send").assertIsNotEnabled()

        compose.onNodeWithText("Message #general").performTextInput("hi there")
        compose.onNodeWithContentDescription("Send").performClick()
        compose.waitForIdle()

        assertEquals(listOf("hi there"), sent)
        compose.onNodeWithText("hi there").assertDoesNotExist()
    }

    @Test
    fun anEmptyChannelSaysSo() {
        compose.setContent {
            ShyTheme(isDark = false) {
                ChannelScreen(title = "#general", state = ChannelState(isLoading = false), onBack = {}, onSend = { true }, onSeen = {})
            }
        }
        compose.onNodeWithText("No messages yet. Say hello.").assertExists()
    }

    @Test
    fun aMessageShowsItsPlainTextNotItsMarkdown() {
        val mention = sampleMessage("1", "Ada", "[@Grace](shy://member/grace) **look**")
        compose.setContent {
            ShyTheme(isDark = false) {
                ChannelScreen(
                    title = "#general",
                    state = ChannelState(messages = listOf(mention), plainTexts = mapOf("1" to "@Grace look"), isLoading = false),
                    onBack = {},
                    onSend = { true },
                    onSeen = {},
                )
            }
        }

        compose.onNodeWithText("@Grace look").assertExists()
        compose.onNodeWithText("shy://member", substring = true).assertDoesNotExist()
    }

    @Test
    fun theNewestMessageIsReportedSeen() {
        val seen = mutableListOf<String>()
        val messages = listOf(sampleMessage("2", "Ada", "newer"), sampleMessage("1", "Ada", "older"))
        compose.setContent {
            ShyTheme(isDark = false) {
                ChannelScreen(
                    title = "#general",
                    state = ChannelState(messages = messages, isLoading = false),
                    onBack = {},
                    onSend = { true },
                    onSeen = { seen += it },
                )
            }
        }
        compose.waitForIdle()

        assertEquals(listOf("2"), seen)
    }

    private fun showMessages(vararg newestFirst: uniffi.shy_core.Message) {
        compose.setContent {
            ShyTheme(isDark = false) {
                ChannelScreen(
                    title = "#general",
                    state = ChannelState(messages = newestFirst.toList(), isLoading = false),
                    onBack = {},
                    onSend = { true },
                    onSeen = {},
                )
            }
        }
    }

    @Test
    fun aMessageShowsItsEditReactionsAndReplies() {
        val thumbs = uniffi.shy_core.Reaction(emoji = "👍", count = 2u, mine = false)
        showMessages(sampleMessage("1", "Ada", "fixed", reactions = listOf(thumbs), replyCount = 3u).copy(isEdited = true))

        compose.onNodeWithText("fixed (edited)").assertExists()
        compose.onNodeWithContentDescription("👍, 2 reactions").assertExists()
        compose.onNodeWithText("3 replies").assertExists()
    }

    @Test
    fun anAgentIsMarkedAndSomeoneWithoutANameStillHasOne() {
        showMessages(
            sampleMessage("2", "", "hello").copy(author = "f00dfeedcafe0000"),
            sampleMessage("1", "Helper", "on it").copy(authorIsBot = true),
        )

        compose.onNodeWithText("Agent").assertExists()
        compose.onNodeWithText("member f00dfe").assertExists()
    }

    @Test
    fun aMessageShownWithoutItsNameStillSaysWhoseItIs() {
        val start = 1_764_230_400_000L
        showMessages(
            sampleMessage("2", "Ada", "second", createdMs = start + 60_000),
            sampleMessage("1", "Ada", "first", createdMs = start),
        )

        compose.onNodeWithText("second").assertContentDescriptionContains("Ada", substring = true)
    }

    @Test
    fun aNewMessageIsSeenOnlyOnceScrolledDownTo() {
        val seen = mutableListOf<String>()
        val older = (1..40).map { sampleMessage("$it", "Ada", "message $it") }.reversed()
        var state by mutableStateOf(ChannelState(messages = older, isLoading = false))
        compose.setContent {
            ShyTheme(isDark = false) {
                ChannelScreen(title = "#general", state = state, onBack = {}, onSend = { true }, onSeen = { seen += it })
            }
        }
        compose.waitForIdle()
        assertEquals(listOf("40"), seen)

        compose.onNode(hasScrollToIndexAction()).performScrollToIndex(older.size)
        state = state.copy(messages = listOf(sampleMessage("41", "Grace", "new")) + older)
        compose.waitForIdle()
        assertEquals(listOf("40"), seen)

        compose.onNode(hasScrollToIndexAction()).performScrollToIndex(0)
        compose.waitForIdle()
        assertEquals(listOf("40", "41"), seen)
    }

    @Test
    fun atTheNewestMessageANewOneComesIntoView() {
        val older = (1..40).map { sampleMessage("$it", "Ada", "message $it") }.reversed()
        var state by mutableStateOf(ChannelState(messages = older, isLoading = false))
        compose.setContent {
            ShyTheme(isDark = false) {
                ChannelScreen(title = "#general", state = state, onBack = {}, onSend = { true }, onSeen = {})
            }
        }
        compose.waitForIdle()

        state = state.copy(messages = listOf(sampleMessage("41", "Grace", "just arrived")) + older)
        compose.waitForIdle()

        compose.onNodeWithText("just arrived").assertIsDisplayed()
    }
}
