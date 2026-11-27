package dev.shynewt.shyspace.features

import androidx.compose.ui.test.junit4.v2.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.performClick
import dev.shynewt.shyspace.features.home.HomeScreen
import dev.shynewt.shyspace.features.home.HomeState
import dev.shynewt.shyspace.ui.theme.ShyTheme
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class HomeScreenTest {
    @get:Rule
    val compose = createComposeRule()

    @Test
    fun theSettingsButtonOpensSettings() {
        var opened = 0
        compose.setContent {
            ShyTheme(isDark = false) {
                HomeScreen(
                    state = HomeState.Ready("w1", "Acme", channels = emptyList(), openFailures = emptyList()),
                    onOpenChannel = { _, _ -> },
                    onOpenSettings = { opened++ },
                    noWorkspaceContent = {},
                )
            }
        }

        compose.onNodeWithContentDescription("Settings").performClick()

        assertEquals(1, opened)
    }
}
