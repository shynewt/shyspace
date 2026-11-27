package dev.shynewt.shyspace.features

import androidx.compose.ui.test.assert
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.LiveRegionMode
import android.content.ClipData
import android.content.ClipboardManager
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.test.junit4.v2.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performImeAction
import androidx.compose.ui.test.performTextInput
import dev.shynewt.shyspace.features.onboarding.LinkScreen
import dev.shynewt.shyspace.features.onboarding.LinkStep
import dev.shynewt.shyspace.features.onboarding.WelcomeScreen
import dev.shynewt.shyspace.ui.theme.ShyTheme
import androidx.compose.material3.Text
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class OnboardingScreensTest {
    @get:Rule
    val compose = createComposeRule()

    /** What the screen asked for, in order. */
    private val asked = mutableListOf<String>()

    @Test
    fun withLinkingTheWelcomeOffersIt() {
        compose.setContent {
            ShyTheme(isDark = true) { WelcomeScreen(onLinkThisPhone = { asked += "link" }, developerTools = null, createError = null) }
        }

        compose.onNodeWithText("I already use shyspace").performClick()

        assertEquals(listOf("link"), asked)
        compose.onNodeWithText("For developers").assertDoesNotExist()
    }

    @Test
    fun beforeLinkingTheWelcomeSaysThereIsNothingYetAndHidesTheToolsUntilAsked() {
        compose.setContent {
            ShyTheme(isDark = true) {
                WelcomeScreen(onLinkThisPhone = null, developerTools = { Text("Create a test workspace") }, createError = null)
            }
        }

        compose.onNodeWithText("No workspaces on this phone yet.").assertExists()
        compose.onNodeWithText("I already use shyspace").assertDoesNotExist()
        compose.onNodeWithText("Create a test workspace").assertDoesNotExist()
        compose.onNodeWithText("For developers").performClick()
        compose.onNodeWithText("Create a test workspace").assertExists()
    }

    private var step by mutableStateOf<LinkStep>(LinkStep.EnterCode())

    /** Every text the code field held, in order. */
    private val changes = mutableListOf<String>()

    private fun showLink() {
        compose.setContent {
            ShyTheme(isDark = false) {
                LinkScreen(
                    step = step,
                    onCodeChange = { changes += it },
                    onCodeSubmitted = { asked += "code $it" },
                    onStartAgain = { asked += "again" },
                    onBack = {},
                )
            }
        }
    }

    @Test
    fun everyChangeToTheFieldIsPassedOnForTheCoreToRead() {
        showLink()

        compose.onNodeWithText("Link code").performTextInput("a")
        compose.onNodeWithText("Link code").performTextInput("b")
        compose.waitForIdle()

        assertEquals(listOf("a", "ab"), changes.filter { it.isNotEmpty() })
        assertEquals(emptyList<String>(), asked)
    }

    @Test
    fun aCodeTypedByHandIsHandedInOnDone() {
        showLink()

        compose.onNodeWithText("Link code").performTextInput("ab")
        compose.onNodeWithText("Link code").performImeAction()

        assertEquals(listOf("code ab"), asked)
    }

    @Test
    fun pasteHandsInTheClipboardAsItIs() {
        RuntimeEnvironment.getApplication().getSystemService(ClipboardManager::class.java)
            .setPrimaryClip(ClipData.newPlainText("code", "  shy-link-5678 \n"))
        showLink()

        compose.onNodeWithText("Paste").performClick()
        compose.waitForIdle()

        assertEquals(listOf("code   shy-link-5678 \n"), asked)
    }

    @Test
    fun aProblemWithTheCodeIsShownInTheCoresWords() {
        step = LinkStep.EnterCode(problem = "This code has run out. Show a new one on your other device.")
        showLink()

        compose.onNodeWithText("This code has run out. Show a new one on your other device.").assert(isAnnounced)
    }

    @Test
    fun whileWaitingItSaysSo() {
        step = LinkStep.Waiting
        showLink()

        compose.onNodeWithText("Waiting for your other device…").assertExists()
        compose.onNodeWithText("Link code").assertDoesNotExist()
    }

    @Test
    fun theCheckCodeIsLargeAndReadInTwoGroups() {
        step = LinkStep.Checking("482 917")
        showLink()

        compose.onNodeWithText("On your other device, tap this number").assertExists()
        compose.onNodeWithContentDescription("482, 917").assert(isAnnounced)
        compose.onNodeWithText("On your other device, tap this number").assert(isAnnounced)
        compose.onNodeWithText("Link code").assertDoesNotExist()
    }

    @Test
    fun anEndedLinkSaysWhyAndCanStartAgain() {
        step = LinkStep.Ended("Your other device stopped the link. Nothing was shared.")
        showLink()

        compose.onNodeWithText("Your other device stopped the link. Nothing was shared.").assert(isAnnounced)
        compose.onNodeWithText("Start again").performClick()

        assertEquals(listOf("again"), asked)
    }

    @Test
    fun whenSomeoneElseUsedTheCodeItSaysWhatToDoOnTheOtherDevice() {
        step = LinkStep.SomeoneElse
        showLink()

        compose.onNodeWithText("Someone else used this code. On your other device, choose 'None of these'.").assert(isAnnounced)
        compose.onNodeWithText("Start again").performClick()

        assertEquals(listOf("again"), asked)
    }

    private val isAnnounced = SemanticsMatcher.expectValue(SemanticsProperties.LiveRegion, LiveRegionMode.Polite)
}
