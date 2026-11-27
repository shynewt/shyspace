package dev.shynewt.shyspace.features.channel

import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.util.Locale
import uniffi.shy_core.Message

/** A row of a channel's message list: a day's heading or a message. */
sealed interface ChannelRow {
    val key: String

    data class Day(val date: LocalDate, override val key: String) : ChannelRow

    /** [isContinuation]: it follows the same person's message closely, so it shows without their name. */
    data class Item(val message: Message, val isContinuation: Boolean) : ChannelRow {
        override val key: String get() = message.id
    }
}

/**
 * The rows for messages given newest first, also newest first, as the list shows them from
 * the bottom up. As on the desktop, a message joins the one before it when the same person
 * wrote it within five minutes on the same day and it isn't a reply.
 */
fun channelRows(newestFirst: List<Message>, zone: ZoneId): List<ChannelRow> {
    val rows = mutableListOf<ChannelRow>()
    // Op order can return to an earlier day, so a heading's key counts its day's earlier ones.
    val headingsSoFar = mutableMapOf<LocalDate, Int>()
    var previous: Message? = null
    for (message in newestFirst.asReversed()) {
        val day = Instant.ofEpochMilli(message.createdMs).atZone(zone).toLocalDate()
        val previousDay = previous?.let { Instant.ofEpochMilli(it.createdMs).atZone(zone).toLocalDate() }
        val startsDay = day != previousDay
        if (startsDay) {
            val earlier = headingsSoFar.getOrDefault(day, 0)
            headingsSoFar[day] = earlier + 1
            rows += ChannelRow.Day(day, "day:$day:$earlier")
        }
        val isContinuation = previous != null && !startsDay && previous.author == message.author &&
            message.replyTo == null && message.createdMs - previous.createdMs < GROUP_WINDOW_MS
        rows += ChannelRow.Item(message, isContinuation)
        previous = message
    }
    return rows.asReversed()
}

private const val GROUP_WINDOW_MS = 5 * 60 * 1000L

/** How a day's heading reads: "Today", "Yesterday", or its date, with the year if it isn't this one. */
sealed interface DayName {
    data object Today : DayName

    data object Yesterday : DayName

    data class Date(val text: String) : DayName
}

fun dayName(date: LocalDate, today: LocalDate, locale: Locale): DayName = when (date) {
    today -> DayName.Today
    today.minusDays(1) -> DayName.Yesterday
    else -> {
        val pattern = if (date.year == today.year) "EEEE, MMMM d" else "EEEE, MMMM d, yyyy"
        DayName.Date(date.format(DateTimeFormatter.ofPattern(pattern, locale)))
    }
}
