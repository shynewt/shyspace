package dev.shynewt.shyspace

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import dev.shynewt.shyspace.ui.theme.ShyTheme

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        val core = (application as ShyspaceApplication).core
        setContent {
            ShyTheme {
                ShyspaceNavigation(core = core)
            }
        }
    }
}
