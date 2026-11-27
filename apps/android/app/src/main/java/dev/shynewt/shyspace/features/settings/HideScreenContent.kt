package dev.shynewt.shyspace.features.settings

import android.view.WindowManager
import androidx.activity.compose.LocalActivity
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect

/** Keeps the window out of screenshots, screen recording and the recent-apps preview while [hides]. */
@Composable
fun HideScreenContent(hides: Boolean) {
    val window = checkNotNull(LocalActivity.current) { "HideScreenContent needs an activity" }.window
    DisposableEffect(window, hides) {
        if (hides) window.addFlags(WindowManager.LayoutParams.FLAG_SECURE)
        onDispose { window.clearFlags(WindowManager.LayoutParams.FLAG_SECURE) }
    }
}
