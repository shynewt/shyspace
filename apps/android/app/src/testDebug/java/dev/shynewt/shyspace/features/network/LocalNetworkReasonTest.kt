package dev.shynewt.shyspace.features.network

import androidx.compose.ui.test.junit4.v2.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import dev.shynewt.shyspace.ui.theme.ShyTheme
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class LocalNetworkReasonTest {
    @get:Rule
    val compose = createComposeRule()

    @Test
    fun theReasonNamesWhatAndroidWillAskForAndLetsYouDecline() {
        val answers = mutableListOf<String>()
        compose.setContent {
            ShyTheme(isDark = false) {
                LocalNetworkReason(onContinue = { answers += "continue" }, onDismiss = { answers += "not now" })
            }
        }

        compose.onNodeWithText("Nearby devices", substring = true).assertExists()
        compose.onNodeWithText("Not now").performClick()
        compose.onNodeWithText("Continue").performClick()

        assertEquals(listOf("not now", "continue"), answers)
    }
}
