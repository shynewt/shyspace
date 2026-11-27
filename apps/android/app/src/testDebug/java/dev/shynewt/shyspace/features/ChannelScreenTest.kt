package dev.shynewt.shyspace.features

import org.robolectric.RuntimeEnvironment
import org.junit.Assert.assertTrue
import kotlinx.coroutines.CompletableDeferred
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.Role
import android.content.ClipboardManager
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
import androidx.compose.ui.test.longClick
import androidx.compose.ui.test.performTextClearance
import androidx.compose.ui.test.performTouchInput
import uniffi.shy_core.Message
import uniffi.shy_core.Reaction
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
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
                    onEdit = { _, _ -> true },
                    onReact = { _, _, _ -> },
                    onDelete = {},
                    onLoadOlder = {},
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
                ChannelScreen(title = "#general", state = ChannelState(isLoading = false), onBack = {}, onSend = { true }, onSeen = {}, onEdit = { _, _ -> true }, onReact = { _, _, _ -> }, onDelete = {}, onLoadOlder = {})
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
                    onEdit = { _, _ -> true },
                    onReact = { _, _, _ -> },
                    onDelete = {},
                    onLoadOlder = {},
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
                    onEdit = { _, _ -> true },
                    onReact = { _, _, _ -> },
                    onDelete = {},
                    onLoadOlder = {},
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
                    onEdit = { _, _ -> true },
                    onReact = { _, _, _ -> },
                    onDelete = {},
                    onLoadOlder = {},
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
                ChannelScreen(title = "#general", state = state, onBack = {}, onSend = { true }, onSeen = { seen += it }, onEdit = { _, _ -> true }, onReact = { _, _, _ -> }, onDelete = {}, onLoadOlder = {})
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
                ChannelScreen(title = "#general", state = state, onBack = {}, onSend = { true }, onSeen = {}, onEdit = { _, _ -> true }, onReact = { _, _, _ -> }, onDelete = {}, onLoadOlder = {})
            }
        }
        compose.waitForIdle()

        state = state.copy(messages = listOf(sampleMessage("41", "Grace", "just arrived")) + older)
        compose.waitForIdle()

        compose.onNodeWithText("just arrived").assertIsDisplayed()
    }

    /** What the screen asked of the channel, in order. */
    private val asked = mutableListOf<String>()

    private var shown by mutableStateOf(ChannelState())
    private var isEditSaved = true
    private var sending: CompletableDeferred<Unit>? = null

    private fun showAsking(vararg newestFirst: Message) {
        shown = ChannelState(messages = newestFirst.toList(), isLoading = false)
        compose.setContent {
            ShyTheme(isDark = false) {
                ChannelScreen(
                    title = "#general",
                    state = shown,
                    onBack = {},
                    onSend = { text ->
                        asked += "send $text"
                        sending?.await()
                        true
                    },
                    onSeen = {},
                    onEdit = { message, text -> asked += "edit $message $text"; isEditSaved },
                    onReact = { message, emoji, active -> asked += "react $message $emoji $active" },
                    onDelete = { message -> asked += "delete $message" },
                    onLoadOlder = { asked += "older" },
                )
            }
        }
    }

    private fun mine(id: String, text: String): Message = sampleMessage(id, "Ada", text).copy(canEdit = true, canDelete = true)

    @Test
    fun aLongPressOffersQuickReactions() {
        showAsking(sampleMessage("1", "Grace", "lunch?"))

        compose.onNodeWithText("lunch?").performTouchInput { longClick() }
        compose.onNodeWithContentDescription("React with 👍").performClick()

        assertEquals(listOf("react 1 👍 true"), asked)
    }

    @Test
    fun onlyYourOwnMessagesOfferEditAndDelete() {
        showAsking(sampleMessage("1", "Grace", "lunch?"))

        compose.onNodeWithText("lunch?").performTouchInput { longClick() }

        compose.onNodeWithText("Copy text").assertExists()
        compose.onNodeWithText("Edit").assertDoesNotExist()
        compose.onNodeWithText("Delete").assertDoesNotExist()
    }

    @Test
    fun tappingYourReactionTakesItBack() {
        val thumbs = Reaction(emoji = "👍", count = 2u, mine = true)
        showAsking(sampleMessage("1", "Grace", "lunch?", reactions = listOf(thumbs)))

        compose.onNodeWithContentDescription("👍, 2 reactions").performClick()

        assertEquals(listOf("react 1 👍 false"), asked)
    }

    @Test
    fun editingPutsTheTextInTheBoxAndSavesIt() {
        showAsking(mine("1", "helo"))

        compose.onNodeWithText("helo").performTouchInput { longClick() }
        compose.onNodeWithText("Edit").performClick()
        compose.onNodeWithText("Editing message").assertExists()
        compose.onNodeWithContentDescription("Message #general").performTextClearance()
        compose.onNodeWithContentDescription("Message #general").performTextInput("hello")
        compose.onNodeWithContentDescription("Save").performClick()
        compose.waitForIdle()

        assertEquals(listOf("edit 1 hello"), asked)
        compose.onNodeWithText("Editing message").assertDoesNotExist()
    }

    @Test
    fun deletingAsksFirst() {
        showAsking(mine("1", "oops"))

        compose.onNodeWithText("oops").performTouchInput { longClick() }
        compose.onNodeWithText("Delete").performClick()
        compose.onNodeWithText("Delete this message?").assertExists()
        assertEquals(emptyList<String>(), asked)

        compose.onNodeWithText("Delete message").performClick()

        assertEquals(listOf("delete 1"), asked)
    }

    private fun box() = compose.onNodeWithContentDescription("Message #general")

    private fun startEditing(text: String) {
        compose.onNodeWithText(text).performTouchInput { longClick() }
        compose.onNodeWithText("Edit").performClick()
    }

    @Test
    fun stoppingAnEditBringsBackWhatYouWereTyping() {
        showAsking(mine("1", "helo"))
        box().performTextInput("half a thought")

        startEditing("helo")
        compose.onNodeWithText("half a thought").assertDoesNotExist()
        compose.onNodeWithContentDescription("Stop editing").performClick()

        box().assert(hasText("half a thought"))
        assertEquals(emptyList<String>(), asked)
    }

    @Test
    fun savingAnEditBringsBackWhatYouWereTyping() {
        showAsking(mine("1", "helo"))
        box().performTextInput("half a thought")

        startEditing("helo")
        box().performTextClearance()
        box().performTextInput("hello")
        compose.onNodeWithContentDescription("Save").performClick()
        compose.waitForIdle()

        assertEquals(listOf("edit 1 hello"), asked)
        box().assert(hasText("half a thought"))
    }

    @Test
    fun aFailedSaveKeepsTheEditOpenWithItsText() {
        isEditSaved = false
        showAsking(mine("1", "helo"))

        startEditing("helo")
        box().performTextClearance()
        box().performTextInput("hello")
        compose.onNodeWithContentDescription("Save").performClick()
        compose.waitForIdle()

        compose.onNodeWithText("Editing message").assertExists()
        box().assert(hasText("hello"))
    }

    @Test
    fun aMessageDeletedWhileYouEditItEndsTheEditAndKeepsEverythingTyped() {
        showAsking(mine("1", "helo"))
        box().performTextInput("half a thought")
        startEditing("helo")
        box().performTextClearance()
        box().performTextInput("hello")

        shown = shown.copy(messages = emptyList())
        compose.waitForIdle()

        compose.onNodeWithText("Editing message").assertDoesNotExist()
        box().assert(hasText("half a thought\n\nhello"))
    }

    @Test
    fun aDoubleTapOnSendSendsOnce() {
        val gate = CompletableDeferred<Unit>()
        sending = gate
        showAsking()
        box().performTextInput("hi")

        compose.onNodeWithContentDescription("Send").performClick()
        compose.onNodeWithContentDescription("Send").performClick()
        gate.complete(Unit)
        compose.waitForIdle()

        assertEquals(listOf("send hi"), asked)
    }

    @Test
    fun copiedTextIsMarkedSensitiveSoTheSystemPreviewHidesIt() {
        showAsking(sampleMessage("1", "Grace", "lunch?"))

        compose.onNodeWithText("lunch?").performTouchInput { longClick() }
        compose.onNodeWithText("Copy text").performClick()
        compose.waitForIdle()

        val clipboard = RuntimeEnvironment.getApplication().getSystemService(ClipboardManager::class.java)
        val clip = clipboard.primaryClip
        assertEquals("lunch?", clip?.getItemAt(0)?.text?.toString())
        assertTrue(clip?.description?.extras?.getBoolean("android.content.extra.IS_SENSITIVE") == true)
    }

    @Test
    fun theDeleteQuestionQuotesTheTextAsShown() {
        showAsking()
        shown = shown.copy(messages = listOf(mine("1", "**bold** plan")), plainTexts = mapOf("1" to "bold plan"))
        compose.waitForIdle()

        compose.onNodeWithText("bold plan").performTouchInput { longClick() }
        compose.onNodeWithText("Delete").performClick()

        compose.onAllNodesWithText("bold plan").assertCountEquals(2)
        compose.onNodeWithText("**bold** plan").assertDoesNotExist()
    }

    @Test
    fun cancellingADeleteDeletesNothing() {
        showAsking(mine("1", "oops"))

        compose.onNodeWithText("oops").performTouchInput { longClick() }
        compose.onNodeWithText("Delete").performClick()
        compose.onNodeWithText("Cancel").performClick()

        compose.onNodeWithText("Delete this message?").assertDoesNotExist()
        assertEquals(emptyList<String>(), asked)
    }

    @Test
    fun theSheetTakesBackYourReaction() {
        val thumbs = Reaction(emoji = "👍", count = 1u, mine = true)
        showAsking(sampleMessage("1", "Grace", "lunch?", reactions = listOf(thumbs)))

        compose.onNodeWithText("lunch?").performTouchInput { longClick() }
        compose.onNodeWithContentDescription("Remove your 👍").performClick()

        assertEquals(listOf("react 1 👍 false"), asked)
    }

    @Test
    fun theSheetIsNamedAndItsRowsAreButtons() {
        showAsking(mine("1", "helo"))

        compose.onNodeWithText("helo").performTouchInput { longClick() }

        compose.onNode(SemanticsMatcher.expectValue(SemanticsProperties.PaneTitle, "Message actions")).assertExists()
        for (action in listOf("Copy text", "Edit", "Delete")) {
            compose.onNodeWithText(action).assert(SemanticsMatcher.expectValue(SemanticsProperties.Role, Role.Button))
        }
    }

    @Test
    fun reachingTheOldestLoadedMessageAsksForOlderOnesOnlyWhileThereAreMore() {
        val loaded = (1..60).map { sampleMessage("$it", "Ada", "message $it") }.reversed()
        var requests = 0
        var state by mutableStateOf(ChannelState(messages = loaded, isLoading = false, hasOlder = true))
        compose.setContent {
            ShyTheme(isDark = false) {
                ChannelScreen(
                    title = "#general", state = state, onBack = {}, onSend = { true }, onSeen = {},
                    onEdit = { _, _ -> true }, onReact = { _, _, _ -> }, onDelete = {}, onLoadOlder = { requests++ },
                )
            }
        }
        compose.waitForIdle()
        assertEquals(0, requests)

        compose.onNode(hasScrollToIndexAction()).performScrollToIndex(loaded.size)
        compose.waitForIdle()
        assertTrue(requests > 0)

        val asked = requests
        state = state.copy(hasOlder = false)
        compose.onNode(hasScrollToIndexAction()).performScrollToIndex(0)
        compose.onNode(hasScrollToIndexAction()).performScrollToIndex(loaded.size)
        compose.waitForIdle()
        assertEquals(asked, requests)
    }
}
