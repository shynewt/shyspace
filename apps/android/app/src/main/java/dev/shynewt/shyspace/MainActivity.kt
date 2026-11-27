package dev.shynewt.shyspace

import android.os.Bundle
import android.view.WindowManager
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.runtime.getValue
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import dev.shynewt.shyspace.features.settings.HideScreenContent
import dev.shynewt.shyspace.ui.theme.ShyTheme

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        val app = application as ShyspaceApplication
        // Before the first frame too, not only once composition runs.
        if (app.settings.hidesScreenContent.value) window.addFlags(WindowManager.LayoutParams.FLAG_SECURE)
        setContent {
            val hidesScreenContent by app.settings.hidesScreenContent.collectAsStateWithLifecycle()
            HideScreenContent(hidesScreenContent)
            ShyTheme {
                ShyspaceNavigation(core = app.core, settings = app.settings, linkKeeper = app.linkKeeper)
            }
        }
    }
}
