package dev.shynewt.shyspace.features.channel

import java.time.LocalDate
import java.time.ZoneId
import java.time.ZonedDateTime
import java.util.Locale
import org.junit.Assert.assertEquals
import org.junit.Test
import uniffi.shy_core.Message

class ChannelRowsTest {
    private val zone = ZoneId.of("Europe/Madrid")
    private fun at(day: Int, hour: Int, minute: Int) =
        ZonedDateTime.of(2026, 11, day, hour, minute, 0, 0, zone).toInstant().toEpochMilli()

    private fun message(id: String, author: String, ms: Long, replyTo: String? = null) = Message(
        id = id, channel = "general", author = author, device = "$author-phone", authorName = author,
        authorIsBot = false, text = id, replyTo = replyTo, createdMs = ms, lamport = 1u, isEdited = false,
        canEdit = false, canDelete = false, mentionsMe = false, saved = null, replyCount = 0u, lastReplyMs = null,
        recentReplyAuthors = emptyList(), lastReplyAuthor = null, lastReplyPreview = null, isFollowing = false,
        unreadReplies = 0u, reactions = emptyList(), files = emptyList(),
    )

    /** Rows as text, oldest first: "day 2026-11-26", "a" (starts a group) or "  b" (continues one). */
    private fun describe(newestFirst: List<Message>) = channelRows(newestFirst, zone).asReversed().map { row ->
        when (row) {
            is ChannelRow.Day -> "day ${row.date}"
            is ChannelRow.Item -> if (row.isContinuation) "  ${row.message.id}" else row.message.id
        }
    }

    @Test
    fun messagesByOnePersonCloseTogetherFormAGroupUnderTheirDay() {
        val oldestFirst = listOf(
            message("a1", "ada", at(26, 9, 0)),
            message("a2", "ada", at(26, 9, 4)),
            message("g1", "grace", at(26, 9, 5)),
            message("a3", "ada", at(26, 9, 6)),
            message("a4", "ada", at(26, 9, 12)),
            message("a5", "ada", at(27, 8, 0)),
        )
        assertEquals(
            listOf("day 2026-11-26", "a1", "  a2", "g1", "a3", "a4", "day 2026-11-27", "a5"),
            describe(oldestFirst.reversed()),
        )
    }

    @Test
    fun aReplyStartsItsOwnGroup() {
        val oldestFirst = listOf(message("a1", "ada", at(26, 9, 0)), message("a2", "ada", at(26, 9, 1), replyTo = "x"))
        assertEquals(listOf("day 2026-11-26", "a1", "a2"), describe(oldestFirst.reversed()))
    }

    @Test
    fun aDayThatComesBackGetsAHeadingWithItsOwnKey() {
        // Op order can put a late-synced message from an earlier day after a newer one.
        val oldestFirst = listOf(
            message("a1", "ada", at(26, 9, 0)),
            message("a2", "ada", at(27, 9, 0)),
            message("g1", "grace", at(26, 18, 0)),
            message("a3", "ada", at(27, 10, 0)),
        )
        val rows = channelRows(oldestFirst.reversed(), zone)

        assertEquals(rows.size, rows.map { it.key }.toSet().size)
        assertEquals(
            listOf("day 2026-11-26", "a1", "day 2026-11-27", "a2", "day 2026-11-26", "g1", "day 2026-11-27", "a3"),
            describe(oldestFirst.reversed()),
        )
    }

    @Test
    fun aDayIsNamedRelativeToToday() {
        val today = LocalDate.of(2026, 11, 27)
        val english = Locale.US
        assertEquals(DayName.Today, dayName(today, today, english))
        assertEquals(DayName.Yesterday, dayName(today.minusDays(1), today, english))
        assertEquals(DayName.Date("Monday, November 23"), dayName(LocalDate.of(2026, 11, 23), today, english))
        assertEquals(DayName.Date("Wednesday, December 31, 2025"), dayName(LocalDate.of(2025, 12, 31), today, english))
    }
}
