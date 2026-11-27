package dev.shynewt.shyspace.features

import android.content.ClipboardManager
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.semantics.ProgressBarRangeInfo
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.v2.createComposeRule
import androidx.compose.ui.test.longClick
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextClearance
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.performTouchInput
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.LifecycleRegistry
import androidx.lifecycle.compose.LocalLifecycleOwner
import dev.shynewt.shyspace.features.channel.ThreadScreen
import dev.shynewt.shyspace.features.channel.ThreadState
import dev.shynewt.shyspace.sampleMessage
import dev.shynewt.shyspace.ui.theme.ShyTheme
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import uniffi.shy_core.Message
import uniffi.shy_core.Reaction
import java.time.LocalDate
import java.time.ZoneId

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class ThreadScreenTest {
    @get:Rule
    val compose = createComposeRule()

    /** What the screen asked of the thread, in order. */
    private val asked = mutableListOf<String>()
    private var shown by mutableStateOf(ThreadState())

    /** A lifecycle the test moves by hand, as the screen goes behind another app and back. */
    private class ScreenLifecycle : LifecycleOwner {
        val registry = LifecycleRegistry.createUnsafe(this)
        override val lifecycle: Lifecycle get() = registry
    }

    private val screen = ScreenLifecycle().apply { registry.currentState = Lifecycle.State.RESUMED }

    private val root = sampleMessage("1", "Grace", "lunch?", replyCount = 2u)

    private fun mine(id: String, text: String): Message = sampleMessage(id, "Ada", text).copy(canEdit = true, canDelete = true)

    private fun showThread(state: ThreadState) {
        shown = state
        compose.setContent {
            CompositionLocalProvider(LocalLifecycleOwner provides screen) {
                ShyTheme(isDark = false) {
                    ThreadScreen(
                        channelTitle = "#general",
                        state = shown,
                        onBack = {},
                        onReply = { text -> asked += "reply $text"; true },
                        onSeen = { asked += "seen $it" },
                        onEdit = { message, text -> asked += "edit $message $text"; true },
                        onReact = { message, emoji, active -> asked += "react $message $emoji $active" },
                        onDelete = { asked += "delete $it" },
                    )
                }
            }
        }
    }

    private fun box() = compose.onNodeWithContentDescription("Reply…")

    @Test
    fun aThreadShowsItsFirstMessageAndRepliesAndRepliesInIt() {
        showThread(ThreadState(root = root, replies = listOf(sampleMessage("3", "Ada", "noon works"), sampleMessage("2", "Linus", "yes")), isLoading = false))

        compose.onNodeWithText("in #general").assertExists()
        compose.onNodeWithText("lunch?").assertExists()
        compose.onNodeWithText("noon works").assertExists()
        box().performTextInput("see you there")
        compose.onNodeWithContentDescription("Send").performClick()
        compose.waitForIdle()

        assertEquals(listOf("seen 3", "reply see you there"), asked)
    }

    @Test
    fun insideTheThreadTheRepliesLineIsADividerNotALink() {
        showThread(ThreadState(root = root, replies = listOf(sampleMessage("3", "Ada", "noon"), sampleMessage("2", "Linus", "yes")), isLoading = false))

        compose.onAllNodesWithText("2 replies").assertCountEquals(1)
        compose.onNodeWithText("2 replies").assert(SemanticsMatcher.keyNotDefined(SemanticsActions.OnClick))
    }

    @Test
    fun repliesFromAnotherDayGetADayDivider() {
        val today = LocalDate.now().atTime(9, 0).atZone(ZoneId.systemDefault()).toInstant().toEpochMilli()
        val yesterday = today - 24 * 60 * 60 * 1000L
        showThread(
            ThreadState(
                root = root.copy(createdMs = yesterday),
                replies = listOf(sampleMessage("2", "Linus", "yes", createdMs = today)),
                isLoading = false,
            ),
        )

        compose.onNodeWithText("Yesterday").assertExists()
        compose.onNodeWithText("Today").assertExists()
    }

    @Test
    fun theNewestReplyIsSeenOnlyOnceTheScreenIsBackInFront() {
        screen.registry.currentState = Lifecycle.State.CREATED
        showThread(ThreadState(root = root, replies = listOf(sampleMessage("2", "Linus", "yes")), isLoading = false))
        compose.waitForIdle()
        assertEquals(emptyList<String>(), asked)

        compose.runOnIdle { screen.registry.currentState = Lifecycle.State.RESUMED }
        compose.waitForIdle()

        assertEquals(listOf("seen 2"), asked)
    }

    @Test
    fun aReplyCanBeEditedInTheThread() {
        showThread(ThreadState(root = root, replies = listOf(mine("2", "helo")), isLoading = false))
        compose.waitForIdle()
        asked.clear()

        compose.onNodeWithText("helo").performTouchInput { longClick() }
        compose.onNodeWithText("Edit").performClick()
        box().performTextClearance()
        box().performTextInput("hello")
        compose.onNodeWithContentDescription("Save").performClick()
        compose.waitForIdle()

        assertEquals(listOf("edit 2 hello"), asked)
    }

    @Test
    fun aReplyDeletedWhileYouEditItEndsTheEditAndKeepsTheText() {
        showThread(ThreadState(root = root, replies = listOf(mine("2", "helo")), isLoading = false))
        compose.onNodeWithText("helo").performTouchInput { longClick() }
        compose.onNodeWithText("Edit").performClick()
        box().performTextClearance()
        box().performTextInput("hello")

        shown = shown.copy(replies = emptyList())
        compose.waitForIdle()

        compose.onNodeWithText("Editing message").assertDoesNotExist()
        box().assert(hasText("hello"))
    }

    @Test
    fun aReplyCanBeDeletedReactedToAndCopiedInTheThread() {
        val thumbs = Reaction(emoji = "👍", count = 1u, mine = true)
        showThread(ThreadState(root = root, replies = listOf(mine("2", "oops").copy(reactions = listOf(thumbs))), isLoading = false))
        compose.waitForIdle()
        asked.clear()

        compose.onNodeWithText("oops").performTouchInput { longClick() }
        compose.onNodeWithContentDescription("Remove your 👍").performClick()
        compose.onNodeWithText("oops").performTouchInput { longClick() }
        compose.onNodeWithText("Copy text").performClick()
        compose.onNodeWithText("oops").performTouchInput { longClick() }
        compose.onNodeWithText("Delete").performClick()
        compose.onNodeWithText("Delete message").performClick()
        compose.waitForIdle()

        assertEquals(listOf("react 2 👍 false", "delete 2"), asked)
        val clip = RuntimeEnvironment.getApplication().getSystemService(ClipboardManager::class.java).primaryClip
        assertEquals("oops", clip?.getItemAt(0)?.text?.toString())
        assertTrue(clip?.description?.extras?.getBoolean("android.content.extra.IS_SENSITIVE") == true)
    }

    @Test
    fun insideTheThreadTheSheetOffersNoThreadToReplyIn() {
        showThread(ThreadState(root = root, replies = emptyList(), isLoading = false))

        compose.onNodeWithText("lunch?").performTouchInput { longClick() }

        compose.onNodeWithText("Copy text").assertExists()
        compose.onNodeWithText("Reply in thread").assertDoesNotExist()
    }

    @Test
    fun aThreadShowsItIsLoadingBeforeItsFirstLoad() {
        showThread(ThreadState())

        compose.onNode(SemanticsMatcher.expectValue(SemanticsProperties.ProgressBarRangeInfo, ProgressBarRangeInfo.Indeterminate))
            .assertExists()
    }

    @Test
    fun aDeletedThreadSaysSoCalmlyAndTakesNoReplies() {
        showThread(ThreadState(root = root, isLoading = false))

        shown = ThreadState(isLoading = false, isGone = true)
        compose.waitForIdle()

        compose.onNodeWithText("This thread was deleted.").assertExists()
        box().assertDoesNotExist()
    }
}
