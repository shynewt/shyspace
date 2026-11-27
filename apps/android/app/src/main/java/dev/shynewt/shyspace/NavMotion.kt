package dev.shynewt.shyspace

import androidx.compose.animation.ContentTransform
import androidx.compose.animation.EnterTransition
import androidx.compose.animation.ExitTransition
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.scaleIn
import androidx.compose.animation.scaleOut
import androidx.compose.animation.slideInHorizontally
import androidx.compose.animation.slideOutHorizontally
import androidx.compose.animation.togetherWith
import dev.shynewt.shyspace.ui.LastInput
import dev.shynewt.shyspace.ui.theme.ShyMotion

/*
 * Moving between a list and what it opens happens many times a day, so it takes at most
 * ShyMotion.fastMillis, and only fades and moves (transform and opacity), never resizes.
 */

/** Opening a deeper screen: it fades in from a little to the right while the list fades out. */
internal fun forwardMotion(): ContentTransform {
    val enter = fadeIn(tween(ShyMotion.fastMillis, easing = ShyMotion.easeOut)) +
        slideInHorizontally(tween(ShyMotion.fastMillis, easing = ShyMotion.easeOut)) { width -> width / SLIDE_FRACTION }
    val exit = fadeOut(tween(ShyMotion.exitMillis, easing = ShyMotion.easeOut))
    return enter togetherWith exit
}

/** Back by the button or a key: the reverse of opening. */
internal fun backMotion(): ContentTransform {
    val enter = fadeIn(tween(ShyMotion.fastMillis, easing = ShyMotion.easeOut))
    val exit = fadeOut(tween(ShyMotion.exitMillis, easing = ShyMotion.easeOut)) +
        slideOutHorizontally(tween(ShyMotion.exitMillis, easing = ShyMotion.easeOut)) { width -> width / SLIDE_FRACTION }
    return enter togetherWith exit
}

/**
 * Back by the gesture, as Android's predictive back guide asks of a full-screen surface: while
 * the finger moves, the screen shrinks toward 90% and is gone by 35% of the way; the one behind
 * settles from 110% and fades in from there. The gesture drives it, and letting go finishes it
 * within ShyMotion.fastMillis.
 */
internal fun predictiveBackMotion(): ContentTransform {
    val total = ShyMotion.fastMillis
    val swap = total * 35 / 100
    val enter = fadeIn(tween(total - swap, delayMillis = swap, easing = ShyMotion.easeOut)) +
        scaleIn(tween(total, easing = ShyMotion.easeOut), initialScale = 1.1f)
    val exit = fadeOut(tween(swap, easing = ShyMotion.easeOut)) +
        scaleOut(tween(total, easing = ShyMotion.easeOut), targetScale = 0.9f)
    return enter togetherWith exit
}

/** Opening a deeper screen: at once after a key, otherwise [forwardMotion]. */
internal fun openMotion(lastInput: LastInput): ContentTransform = if (lastInput.wasAKey) atOnce() else forwardMotion()

/** Back by the on-screen button or a key: at once after a key, otherwise [backMotion]. */
internal fun closeMotion(lastInput: LastInput): ContentTransform = if (lastInput.wasAKey) atOnce() else backMotion()

/** After a key: the new screen replaces the old at once. */
internal fun atOnce(): ContentTransform = EnterTransition.None togetherWith ExitTransition.None

/** How far the slide starts or ends off its place: a hint of direction, not a sweep. */
private const val SLIDE_FRACTION = 16
