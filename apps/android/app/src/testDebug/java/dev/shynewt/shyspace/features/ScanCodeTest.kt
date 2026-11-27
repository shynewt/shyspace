package dev.shynewt.shyspace.features

import androidx.activity.compose.LocalActivityResultRegistryOwner
import androidx.activity.result.ActivityResultRegistry
import androidx.activity.result.ActivityResultRegistryOwner
import androidx.activity.result.contract.ActivityResultContract
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.test.junit4.v2.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.core.app.ActivityOptionsCompat
import dev.shynewt.shyspace.features.onboarding.ScanCodeButton
import dev.shynewt.shyspace.ui.theme.ShyTheme
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class ScanCodeTest {
    @get:Rule
    val compose = createComposeRule()

    /** Android's permission dialog, answered at once with [isGranted]; counts how often it was asked. */
    private class PermissionAnswer(private val isGranted: Boolean) : ActivityResultRegistryOwner {
        var asked = 0

        override val activityResultRegistry = object : ActivityResultRegistry() {
            override fun <I, O> onLaunch(requestCode: Int, contract: ActivityResultContract<I, O>, input: I, options: ActivityOptionsCompat?) {
                asked++
                dispatchResult(requestCode, isGranted)
            }
        }
    }

    private fun show(answer: PermissionAnswer) {
        compose.setContent {
            CompositionLocalProvider(LocalActivityResultRegistryOwner provides answer) {
                ShyTheme(isDark = false) { ScanCodeButton(onCode = {}) }
            }
        }
    }

    @Test
    fun theCameraIsAskedForOnlyWhenScanIsTapped() {
        val answer = PermissionAnswer(isGranted = false)
        show(answer)
        compose.waitForIdle()
        assertEquals(0, answer.asked)

        compose.onNodeWithText("Scan code").performClick()

        assertEquals(1, answer.asked)
    }

    @Test
    fun refusingTheCameraSaysToPasteInsteadAndOpensNoCamera() {
        show(PermissionAnswer(isGranted = false))

        compose.onNodeWithText("Scan code").performClick()

        compose.onNodeWithText("Camera access is off, so paste the code instead.").assertExists()
        compose.onNodeWithText("Point the camera at the code on your other device.").assertDoesNotExist()
        compose.onNodeWithContentDescription("Close").assertDoesNotExist()
    }
}
