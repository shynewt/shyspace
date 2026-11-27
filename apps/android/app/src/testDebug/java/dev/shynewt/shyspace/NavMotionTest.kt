package dev.shynewt.shyspace

import androidx.compose.ui.unit.dp
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.focusable
import org.robolectric.annotation.GraphicsMode
import dev.shynewt.shyspace.ui.recordsInput
import androidx.compose.ui.test.pressKey
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.graphics.toPixelMap
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.Alignment
import androidx.compose.runtime.Composable
import androidx.compose.material3.IconButton
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.Text
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.junit4.v2.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import androidx.navigation3.runtime.NavEntry
import androidx.navigation3.ui.NavDisplay
import androidx.navigationevent.DirectNavigationEventInput
import androidx.navigationevent.NavigationEvent
import androidx.navigationevent.NavigationEventDispatcher
import androidx.navigationevent.compose.LocalNavigationEventDispatcherOwner
import dev.shynewt.shyspace.ui.LastInput
import dev.shynewt.shyspace.ui.theme.ShyMotion
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * Opening a channel and going back happen many times a day, so neither may take long; after a
 * key, they happen at once. The screens fill the window, as real ones do, so slides show.
 */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [35])
class NavMotionTest {
    @get:Rule
    val compose = createComposeRule()

    private val backStack = mutableStateListOf("List")
    private val lastInput = LastInput()
    private var dispatcher: NavigationEventDispatcher? = null

    /** Something on the channel that takes keys, as its message box does. */
    private val keysFocus = FocusRequester()

    private fun show() {
        compose.mainClock.autoAdvance = false
        compose.setContent {
            dispatcher = LocalNavigationEventDispatcherOwner.current?.navigationEventDispatcher
            Box(Modifier.fillMaxSize().recordsInput(lastInput)) {
                NavDisplay(
                    backStack = backStack,
                    onBack = { backStack.removeLastOrNull() },
                    transitionSpec = { openMotion(lastInput) },
                    popTransitionSpec = { closeMotion(lastInput) },
                    predictivePopTransitionSpec = { predictiveBackMotion() },
                    entryProvider = { key -> NavEntry(key) { TestScreen(key) } },
                )
            }
        }
        compose.mainClock.advanceTimeByFrame()
    }

    /** A screen filling the window in its own colour, the channel with a top bar's back arrow. */
    @Composable
    private fun TestScreen(key: String) {
        Box(Modifier.fillMaxSize().background(if (key == "List") Color.Blue else Color.Red)) {
            Text(key)
            if (key == "Channel") {
                Box(Modifier.size(1.dp).align(Alignment.BottomCenter).focusRequester(keysFocus).focusable())
                IconButton(onClick = { backStack.removeLastOrNull() }, modifier = Modifier.align(Alignment.Center)) { Text("Back") }
            }
        }
    }

    private fun isShown(text: String) = compose.onAllNodesWithText(text).fetchSemanticsNodes().isNotEmpty()

    private fun boundsOf(text: String) = compose.onNodeWithText(text).fetchSemanticsNode().boundsInRoot

    /** Moves the clock frame by frame until [text] shows: the motion starts there. */
    private fun advanceUntilShown(text: String) {
        repeat(START_FRAMES) {
            if (isShown(text)) return
            compose.mainClock.advanceTimeByFrame()
        }
        compose.onNodeWithText(text).assertExists()
    }

    @Test
    fun openingADeeperScreenIsDoneWithinTheFastDuration() {
        show()

        compose.runOnIdle { backStack.add("Channel") }
        advanceUntilShown("Channel")
        assertTrue("it slides in from the right", boundsOf("Channel").left > 0f)
        advanceFrames(framesFor(ShyMotion.fastMillis) + SLACK_FRAMES)

        compose.onNodeWithText("List").assertDoesNotExist()
        assertEquals(0f, boundsOf("Channel").left)
    }

    @Test
    fun goingBackIsDoneWithinTheFastDuration() {
        backStack.add("Channel")
        show()

        compose.runOnIdle { backStack.removeLastOrNull() }
        advanceUntilShown("List")
        advanceFrames(framesFor(ShyMotion.fastMillis) + SLACK_FRAMES)

        compose.onNodeWithText("Channel").assertDoesNotExist()
        assertEquals(0f, boundsOf("List").left)
    }

    @Test
    fun afterAKeyScreensChangeAtOnce() {
        show()
        lastInput.keyPressed()

        compose.runOnIdle { backStack.add("Channel") }
        advanceUntilShown("Channel")
        assertEquals("no slide: it arrives in place", 0f, boundsOf("Channel").left)
        advanceFrames(SWAP_FRAMES)
        compose.onNodeWithText("List").assertDoesNotExist()

        compose.runOnIdle { backStack.removeLastOrNull() }
        advanceUntilShown("List")
        assertEquals("no slide: it leaves in place", 0f, boundsOf("Channel").left)
        advanceFrames(SWAP_FRAMES)
        compose.onNodeWithText("Channel").assertDoesNotExist()
    }

    @Test
    fun theBackGestureShrinksTheScreenUnderTheFingerAndFinishesQuickly() {
        backStack.add("Channel")
        show()
        val fullWidth = boundsOf("Channel").width
        val gesture = DirectNavigationEventInput()
        compose.runOnIdle { checkNotNull(dispatcher) { "the activity offers back events" }.addInput(gesture) }

        startHalfWayBack(gesture)
        val shrunk = boundsOf("Channel")
        assertTrue("half way, it shrinks toward 90%: ${shrunk.width} of $fullWidth", shrunk.width in fullWidth * 0.88f..fullWidth * 0.97f)
        assertTrue("it shrinks toward its centre", shrunk.left > 0f)

        compose.runOnIdle { gesture.backCompleted() }
        advanceFrames(framesFor(ShyMotion.fastMillis) + SLACK_FRAMES)

        compose.onNodeWithText("Channel").assertDoesNotExist()
        compose.onNodeWithText("List").assertExists()
    }

    @Test
    fun aCancelledBackGestureLeavesTheScreenAsItWas() {
        backStack.add("Channel")
        show()
        val fullWidth = boundsOf("Channel").width
        val gesture = DirectNavigationEventInput()
        compose.runOnIdle { checkNotNull(dispatcher) { "the activity offers back events" }.addInput(gesture) }

        startHalfWayBack(gesture)
        compose.runOnIdle { gesture.backCancelled() }
        advanceFrames(framesFor(ShyMotion.fastMillis) + SLACK_FRAMES)

        compose.onNodeWithText("List").assertDoesNotExist()
        assertEquals(fullWidth, boundsOf("Channel").width)
        assertEquals(listOf("List", "Channel"), backStack.toList())
    }

    /**
     * A back swipe from the left edge, held half way. The navigation sets a swipe up over a few
     * frames and drops progress sent before that, so the steps are spaced as on a phone.
     */
    private fun startHalfWayBack(gesture: DirectNavigationEventInput) {
        compose.runOnIdle { gesture.backStarted(NavigationEvent(touchX = 0f, touchY = 0f, progress = 0f, swipeEdge = NavigationEvent.EDGE_LEFT)) }
        advanceFrames(3)
        compose.runOnIdle { gesture.backProgressed(NavigationEvent(touchX = 100f, touchY = 0f, progress = 0.5f, swipeEdge = NavigationEvent.EDGE_LEFT)) }
        advanceFrames(2)
    }

    private fun advanceFrames(count: Int) = repeat(count) { compose.mainClock.advanceTimeByFrame() }

    private fun framesFor(millis: Int) = (millis + FRAME_MILLIS - 1) / FRAME_MILLIS

    @Test
    fun theOnScreenArrowAnimatesEvenRightAfterAKey() {
        backStack.add("Channel")
        show()
        pressKey(Key.Tab)
        assertTrue(lastInput.wasAKey)

        compose.onNodeWithText("Back").performClick()
        advanceUntilShown("List")
        advanceFrames(SWAP_FRAMES)

        compose.onNodeWithText("Channel").assertExists()
        assertTrue("it slides out", boundsOf("Channel").left > 0f)
    }

    @Test
    fun escapeGoesBackAtOnce() {
        backStack.add("Channel")
        show()

        pressKey(Key.Escape)
        // Android turns an unhandled Esc into Back, which reaches the navigation as this does.
        compose.runOnIdle { backStack.removeLastOrNull() }
        advanceUntilShown("List")
        assertEquals("no slide: it leaves in place", 0f, boundsOf("Channel").left)
        advanceFrames(SWAP_FRAMES)

        compose.onNodeWithText("Channel").assertDoesNotExist()
    }

    @Test
    fun theSwipeSwapsScreensAboutAThirdOfTheWay() {
        backStack.add("Channel")
        show()
        val gesture = DirectNavigationEventInput()
        compose.runOnIdle { checkNotNull(dispatcher) { "the activity offers back events" }.addInput(gesture) }

        val early = colourAt(gesture, progress = 0.1f)
        val atSwap = colourAt(gesture, progress = 0.35f)
        val late = colourAt(gesture, progress = 0.9f)

        assertTrue("early on, the channel still shows: $early", early.green < 0.95f && early.red > 0.95f)
        assertTrue("at the swap, neither screen shows: $atSwap", atSwap.red > 0.95f && atSwap.green > 0.95f && atSwap.blue > 0.95f)
        assertTrue("late, the list shows: $late", late.blue > 0.95f && late.red < 0.5f)
    }

    /** Presses [key] on the channel, letting the clock run meanwhile. */
    private fun pressKey(key: Key) {
        compose.mainClock.autoAdvance = true
        compose.runOnIdle { keysFocus.requestFocus() }
        compose.onRoot().performKeyInput { pressKey(key) }
        compose.waitForIdle()
        compose.mainClock.autoAdvance = false
    }

    /** The centre's colour, with the swipe held at [progress]; the swipe is then let go. */
    private fun colourAt(gesture: DirectNavigationEventInput, progress: Float): Color {
        compose.runOnIdle { gesture.backStarted(NavigationEvent(touchX = 0f, touchY = 0f, progress = 0f, swipeEdge = NavigationEvent.EDGE_LEFT)) }
        advanceFrames(3)
        compose.runOnIdle { gesture.backProgressed(NavigationEvent(touchX = 100f, touchY = 0f, progress = progress, swipeEdge = NavigationEvent.EDGE_LEFT)) }
        advanceFrames(2)
        val pixels = compose.onRoot().captureToImage().toPixelMap()
        val colour = pixels[pixels.width / 2, pixels.height / 3]
        compose.runOnIdle { gesture.backCancelled() }
        advanceFrames(framesFor(ShyMotion.fastMillis) + SLACK_FRAMES)
        return colour
    }

    private companion object {
        const val FRAME_MILLIS = 16

        /** Frames a change may take to start its motion. */
        const val START_FRAMES = 4

        /** The old screen leaves a frame or two after its motion ends. */
        const val SLACK_FRAMES = 2

        /** With no motion, the old screen still takes two frames to go; nothing moves meanwhile. */
        const val SWAP_FRAMES = 2
    }
}
