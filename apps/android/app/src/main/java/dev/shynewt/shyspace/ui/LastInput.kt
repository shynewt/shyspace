package dev.shynewt.shyspace.ui

import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.PointerEventType
import androidx.compose.ui.input.pointer.pointerInput

/**
 * Whether the last input was a key (a keyboard, or a hardware back button) rather than a touch,
 * pen or mouse. What a key changes changes at once, so motion is skipped after one.
 */
@Stable
class LastInput {
    var wasAKey by mutableStateOf(false)
        private set

    fun keyPressed() {
        wasAKey = true
    }

    fun pointed() {
        wasAKey = false
    }
}

/**
 * Records, anywhere inside, a key pressed (before whatever has focus handles it) or a touch,
 * pen or mouse press (before whatever is under it does).
 */
fun Modifier.recordsInput(lastInput: LastInput): Modifier =
    onPreviewKeyEvent { event ->
        if (event.type == KeyEventType.KeyDown) lastInput.keyPressed()
        false
    }.pointerInput(lastInput) {
        awaitPointerEventScope {
            while (true) {
                if (awaitPointerEvent(PointerEventPass.Initial).type == PointerEventType.Press) lastInput.pointed()
            }
        }
    }
