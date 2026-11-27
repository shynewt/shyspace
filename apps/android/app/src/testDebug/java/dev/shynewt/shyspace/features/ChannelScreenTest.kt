package dev.shynewt.shyspace.features

import androidx.compose.ui.test.assertIsNotEnabled
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
