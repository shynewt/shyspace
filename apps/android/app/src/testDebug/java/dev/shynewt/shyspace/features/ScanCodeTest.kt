package dev.shynewt.shyspace.features

import org.robolectric.Shadows.shadowOf
import org.robolectric.RuntimeEnvironment
import org.junit.Before
import dev.shynewt.shyspace.features.onboarding.firstLinkCode
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.Composable
import androidx.compose.material3.Text
import android.content.pm.PackageManager
import android.Manifest
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

    /** What the button handed on. */
    private val found = mutableListOf<String>()

    @Before
    fun phoneHasACamera() {
        shadowOf(RuntimeEnvironment.getApplication().packageManager).setSystemFeature(PackageManager.FEATURE_CAMERA_ANY, true)
    }

    private fun grantCamera() = shadowOf(RuntimeEnvironment.getApplication()).grantPermissions(Manifest.permission.CAMERA)

    private fun show(
        answer: PermissionAnswer,
        scanner: @Composable (isLinkCode: (String) -> Boolean, onCode: (String) -> Unit, onUnavailable: () -> Unit, onClose: () -> Unit) -> Unit = { _, _, _, _ -> },
    ) {
        compose.setContent {
            CompositionLocalProvider(LocalActivityResultRegistryOwner provides answer) {
                ShyTheme(isDark = false) { ScanCodeButton(isLinkCode = { it.startsWith("shy-link") }, onCode = { found += it }, scanner = scanner) }
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

        compose.onNodeWithText("Camera access is off, so paste the code instead.").assert(isAnnounced)
        compose.onNodeWithText("Point the camera at the code on your other device.").assertDoesNotExist()
        compose.onNodeWithContentDescription("Close").assertDoesNotExist()
    }

    @Test
    fun aPhoneWithNoCameraShowsNoScanButton() {
        shadowOf(RuntimeEnvironment.getApplication().packageManager).setSystemFeature(PackageManager.FEATURE_CAMERA_ANY, false)

        show(PermissionAnswer(isGranted = true))

        compose.onNodeWithText("Scan code").assertDoesNotExist()
    }

    @Test
    fun aFoundCodeIsHandedOnAndTheCameraCloses() {
        grantCamera()
        show(PermissionAnswer(isGranted = true)) { _, onCode, _, _ ->
            Text("camera")
            LaunchedEffect(Unit) { onCode("shy-link-1234") }
        }

        compose.onNodeWithText("Scan code").performClick()
        compose.waitForIdle()

        assertEquals(listOf("shy-link-1234"), found)
        compose.onNodeWithText("camera").assertDoesNotExist()
    }

    @Test
    fun withNoCameraToUseItSaysToPasteInstead() {
        grantCamera()
        show(PermissionAnswer(isGranted = true)) { _, _, onUnavailable, _ -> LaunchedEffect(Unit) { onUnavailable() } }

        compose.onNodeWithText("Scan code").performClick()
        compose.waitForIdle()

        compose.onNodeWithText("The camera can't scan here, so paste the code instead.").assert(isAnnounced)
        assertEquals(emptyList<String>(), found)
    }

    @Test
    fun onlyALinkCodeAmongAFramesCodesIsTaken() {
        val isLinkCode = { text: String -> text.startsWith("shy-link") }

        assertEquals("shy-link-1234", firstLinkCode(listOf(null, "", "https://example.com", "shy-link-1234"), isLinkCode))
        assertEquals(null, firstLinkCode(listOf("https://example.com", "WIFI:S:home;;"), isLinkCode))
    }

    private val isAnnounced = SemanticsMatcher.expectValue(SemanticsProperties.LiveRegion, LiveRegionMode.Polite)
}
