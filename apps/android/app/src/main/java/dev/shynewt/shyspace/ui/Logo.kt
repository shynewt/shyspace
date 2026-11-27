package dev.shynewt.shyspace.ui

import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.unit.Dp
import dev.shynewt.shyspace.R

/** The app's logo as on the desktop: the launcher icon's planet on its gradient. Decorative. */
@Composable
fun Logo(size: Dp, modifier: Modifier = Modifier) {
    Box(modifier = modifier.size(size).clip(RoundedCornerShape(percent = 30))) {
        Image(painterResource(R.drawable.ic_launcher_background), contentDescription = null, modifier = Modifier.fillMaxSize())
        Image(painterResource(R.mipmap.ic_launcher_foreground), contentDescription = null, modifier = Modifier.fillMaxSize())
    }
}
