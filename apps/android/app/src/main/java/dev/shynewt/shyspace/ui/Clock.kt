package dev.shynewt.shyspace.ui

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.text.format.DateFormat
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.platform.LocalContext
import androidx.core.content.ContextCompat
import java.time.LocalDate
import java.time.ZoneId
import java.util.Date

/** Today and how to write a time, as this phone sets them now. */
class Clock(val zone: ZoneId, val today: LocalDate, private val timeFormat: java.text.DateFormat) {
    /** A moment's time of day, in the phone's 12- or 24-hour style. */
    fun time(ms: Long): String = timeFormat.format(Date(ms))
}

/**
 * The [Clock], made again when the day turns, the time zone changes, or the 24-hour setting or
 * the time is changed, so dates and times on screen never go stale.
 */
@Composable
fun rememberClock(): Clock {
    val context = LocalContext.current
    var clock by remember { mutableStateOf(clockNow(context)) }
    DisposableEffect(context) {
        val receiver = object : BroadcastReceiver() {
            override fun onReceive(context: Context, intent: Intent) {
                val now = clockNow(context)
                // The minute tick only matters when it turns the day.
                if (intent.action != Intent.ACTION_TIME_TICK || now.today != clock.today) clock = now
            }
        }
        val changes = IntentFilter().apply {
            addAction(Intent.ACTION_TIME_TICK)
            addAction(Intent.ACTION_TIME_CHANGED)
            addAction(Intent.ACTION_TIMEZONE_CHANGED)
        }
        ContextCompat.registerReceiver(context, receiver, changes, ContextCompat.RECEIVER_NOT_EXPORTED)
        onDispose { context.unregisterReceiver(receiver) }
    }
    return clock
}

private fun clockNow(context: Context): Clock {
    val zone = ZoneId.systemDefault()
    return Clock(zone, LocalDate.now(zone), DateFormat.getTimeFormat(context))
}
