package dev.shynewt.shyspace.features.settings

import android.view.WindowManager
import androidx.activity.ComponentActivity
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.test.assertIsOff
import androidx.compose.ui.test.isToggleable
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import dev.shynewt.shyspace.ui.theme.ShyTheme
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class SettingsScreenTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    @Test
    fun tappingHideScreenContentTurnsItOn() {
        val changes = mutableListOf<Boolean>()
        compose.setContent {
            ShyTheme(isDark = false) {
                YouScreen(hidesScreenContent = false, onHidesScreenContentChange = { changes += it }, saveFailed = false, bottomBar = {})
            }
        }

        compose.onNode(isToggleable()).assertIsOff()
        compose.onNodeWithText("Hide screen content").performClick()

        assertEquals(listOf(true), changes)
    }

    @Test
    fun aChangeThatWasntSavedIsShown() {
        compose.setContent {
            ShyTheme(isDark = false) {
                YouScreen(hidesScreenContent = true, onHidesScreenContentChange = {}, saveFailed = true, bottomBar = {})
            }
        }

        compose.onNodeWithText("Couldn't save this change", substring = true).assertExists()
    }

    @Test
    fun theWindowIsSecureOnlyWhileContentIsHidden() {
        var hides by mutableStateOf(true)
        compose.setContent { HideScreenContent(hides) }
        val isSecure = { (compose.activity.window.attributes.flags and WindowManager.LayoutParams.FLAG_SECURE) != 0 }

        compose.waitForIdle()
        assertTrue(isSecure())

        hides = false
        compose.waitForIdle()
        assertFalse(isSecure())
    }
}
