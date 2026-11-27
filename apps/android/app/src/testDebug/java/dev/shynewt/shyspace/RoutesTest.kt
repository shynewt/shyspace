package dev.shynewt.shyspace

import org.junit.Assert.assertEquals
import org.junit.Test
import uniffi.shy_core.InboxItem
import uniffi.shy_core.InboxReason

class RoutesTest {
    private fun item(reason: InboxReason, threadRoot: String?) = InboxItem(
        message = sampleMessage("m1", "Grace", "hi").copy(channel = "general"),
        channelName = "general",
        threadRoot = threadRoot,
        reason = reason,
    )

    @Test
    fun aMentionInTheChannelOpensTheChannel() {
        assertEquals(Route.Channel("w1", "general", "#general"), routeTo(item(InboxReason.MENTION, null), "w1", "#general"))
    }

    @Test
    fun aMentionInsideAThreadOpensTheThread() {
        assertEquals(Route.Thread("w1", "general", "root1", "#general"), routeTo(item(InboxReason.MENTION, "root1"), "w1", "#general"))
    }

    @Test
    fun aReplyOpensItsThread() {
        assertEquals(Route.Thread("w1", "general", "root1", "#general"), routeTo(item(InboxReason.THREAD_REPLY, "root1"), "w1", "#general"))
    }
}
