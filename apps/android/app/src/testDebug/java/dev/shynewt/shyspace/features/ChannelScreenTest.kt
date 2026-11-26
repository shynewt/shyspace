package dev.shynewt.shyspace.features

import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.junit4.v2.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import dev.shynewt.shyspace.features.channel.ChannelScreen
import dev.shynewt.shyspace.features.channel.ChannelState
import dev.shynewt.shyspace.ui.theme.ShyTheme
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import uniffi.shy_core.Message

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class ChannelScreenTest {
    @get:Rule
    val compose = createComposeRule()

    private fun message(id: String, author: String, text: String) = Message(
        id = id, channel = "general", author = "ada", device = "ada-phone", authorName = author, authorIsBot = false, text = text,
        replyTo = null, createdMs = 0, lamport = 1u, isEdited = false, canEdit = false, canDelete = false,
        mentionsMe = false, saved = null, replyCount = 0u, lastReplyMs = null, recentReplyAuthors = emptyList(),
        lastReplyAuthor = null, lastReplyPreview = null, isFollowing = false, unreadReplies = 0u,
        reactions = emptyList(), files = emptyList(),
    )

    @Test
    fun showsMessagesAndSendsWhatIsTyped() {
        val sent = mutableListOf<String>()
        compose.setContent {
            ShyTheme(isDark = false) {
                ChannelScreen(
                    title = "#general",
                    state = ChannelState(messages = listOf(message("1", "Ada", "hello")), isLoading = false),
                    onBack = {},
                    onSend = { text -> sent += text; true },
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
                ChannelScreen(title = "#general", state = ChannelState(isLoading = false), onBack = {}, onSend = { true })
            }
        }
        compose.onNodeWithText("No messages yet. Say hello.").assertExists()
    }
}
