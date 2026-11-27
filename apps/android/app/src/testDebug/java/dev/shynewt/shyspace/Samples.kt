package dev.shynewt.shyspace

import uniffi.shy_core.Channel
import uniffi.shy_core.ChannelKind
import uniffi.shy_core.Message
import uniffi.shy_core.NotifyLevel
import uniffi.shy_core.Reaction

/** A message as the core would give it, with only what a screen shows set. */
fun sampleMessage(
    id: String,
    authorName: String,
    text: String,
    createdMs: Long = 0,
    reactions: List<Reaction> = emptyList(),
    replyCount: UInt = 0u,
) = Message(
    id = id, channel = "general", author = authorName.lowercase(), device = "${authorName.lowercase()}-phone",
    authorName = authorName, authorIsBot = false, text = text, replyTo = null, createdMs = createdMs, lamport = 1u,
    isEdited = false, canEdit = false, canDelete = false, mentionsMe = false, saved = null, replyCount = replyCount,
    lastReplyMs = null, recentReplyAuthors = emptyList(), lastReplyAuthor = null, lastReplyPreview = null,
    isFollowing = false, unreadReplies = 0u, reactions = reactions, files = emptyList(),
)

/** A channel this person is in, as the core would give it. */
fun sampleChannel(name: String, unread: UInt = 0u, mentions: UInt = 0u, kind: ChannelKind = ChannelKind.PUBLIC) = Channel(
    id = name, kind = kind, name = name, dmMembers = emptyList(), isMember = true, isMissingKey = false,
    canDelete = false, unread = unread, unreadMentions = mentions, lastRead = null, notify = NotifyLevel.ALL,
)
