package dev.shynewt.shyspace

import androidx.compose.foundation.focusable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.size
import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.test.junit4.v2.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.click
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.pressKey
import androidx.compose.ui.unit.dp
import dev.shynewt.shyspace.ui.LastInput
import dev.shynewt.shyspace.ui.recordsInput
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class LastInputTest {
    @get:Rule
    val compose = createComposeRule()

    @OptIn(ExperimentalComposeUiApi::class)
    @Test
    fun aKeyPressedInsideIsRecordedAndATouchClearsIt() {
        val lastInput = LastInput()
        val focus = FocusRequester()
        compose.setContent {
            Box(Modifier.recordsInput(lastInput)) {
                Box(Modifier.size(10.dp).testTag("row").focusRequester(focus).focusable())
            }
        }
        compose.runOnIdle { focus.requestFocus() }
        assertFalse(lastInput.wasAKey)

        compose.onNodeWithTag("row").performKeyInput { pressKey(Key.Enter) }
        assertTrue(lastInput.wasAKey)

        compose.onNodeWithTag("row").performTouchInput { click() }
        assertFalse(lastInput.wasAKey)
    }
}
