# Decisions

Why things are the way they are. Add an entry when you make a choice that someone might later question. Newest first.

## No link previews (2026-11)
The app never fetches a link to show a preview. Fetching would tell the linked site who is reading what, outside the encryption, and previews are a known way to leak data out of AI agents' context (Slack's MCP server did). Links stay plain.

## Replies are threads, built on reply_to (2026-11)
One reply model: a reply lives in a side-panel thread instead of the channel, like Slack, with Zulip's attention tools (follow and unfollow, following threads you join). Threads are optional, never forced like Zulip topics. Thread replies don't add to a channel's unread count, but a mention in one still does. No new op was needed; a title and a "resolved" mark would need one (#58).

## Mentions are markdown links; @here means @channel for now (2026-11)
A mention is a link in the message text, not a separate op: nothing new to sync or secure, agents read and write them as plain markdown, and copying a message keeps them. Each device decides what mentions it, exactly, even though everything is encrypted. Until presence exists (#42), @here reaches everyone like @channel.

## Editing and deleting: owner or admin, hide and purge (2026-11)
Only authors edit their messages. Owners delete their own channels, messages and pages; the admin can delete anything, to moderate. Anyone may rename a channel (like page titles). Deletes are permanent markers, so they converge in any order; a deleted page's sub-pages move to the top level instead of disappearing with someone else's delete. Delete purges the views and search but can't erase the encrypted log other devices already hold; `security.md` says so.

## Admin recovery: kit plus snapshots in the bucket (2026-11)
A one-off export goes stale after the next membership change, and restoring a stale MLS state can reuse message keys. So the admin's app uploads a fresh encrypted snapshot after every membership change, and the kit only holds the key to it. Messages and pages are left out of the snapshot; a restored device re-downloads them. Real multi-admin stays open (#5).

## Local encryption: SQLCipher plus a device key in the OS keychain (2026-11)
SQLCipher encrypts the whole database, including indexes and the search index, with little code on our side. The key lives in the OS keychain so the data directory alone is useless. Servers without a keychain use an explicit key file (`--key-file`); there is no silent fallback. SQLCipher links the system OpenSSL; macOS release builds need a decision (Linux and macOS come first, Windows later; see #24).

## Invites: single-use, approved, credential-free (2026-11)
A reusable code containing bucket credentials meant anyone who saw it could join and could vandalize the bucket. Invites now carry only presigned links (one upload, one download) that expire after 7 days, and the admin approves each request. Credentials travel inside the MLS-encrypted answer.

## Pages are markdown in one Yjs text (2026-11)
Agents edit pages from Rust (yrs), people from the editor (Yjs). A block editor like BlockNote stores a complex XML tree that would be hard and fragile to edit from Rust. Plain markdown keeps one simple shape for both, makes search and export trivial, and still gives live collaboration.

## Ids carry their author's key prefix (2026-11)
With "earliest creation wins", any member could take over another's message by reusing its id with an older clock. Tying ids to the author removes the conflict entirely.

## Only the admin changes membership (2026-11)
MLS needs commits applied in one agreed order. Without a server, concurrent commits from different members would split the group. One committer avoids that. Several admins is a future problem (see issues).

## MLS for keys, our own envelope for content (2026-11)
MLS messages can only be decrypted once and in order, which doesn't fit a sync log that new members must read from the start. So MLS only agrees on one secret per epoch; content uses keys exported from it. Newcomers get older keys in an admin-sent keyring.

## Two transports: iroh gossip and an S3 bucket (2026-11)
Gossip gives instant delivery when people are online, with NAT traversal. The bucket is a "dumb" mailbox anyone can rent, so nobody has to run a server. Since all ops merge in any order, both can carry the same envelopes.

## Tauri, not egui (2026-11)
The product is mostly rich text. The web ecosystem has mature editors (CodeMirror, Excalidraw later); egui would mean writing those from scratch. Cost: more memory, and WebKitGTK quirks on Linux.

## Rust core shared by every client (2026-11)
One engine for desktop, CLI, agents and later mobile. Crypto and sync are written once.

## Voice huddles before screen sharing; native mobile apps later (2026-11)
Product scope set by the owner. Mobile is wanted mainly for notifications, which will need a small push relay that sees no content.

## Presence over gossip only; status as an op (2026-11)
Presence changes every few minutes and is worthless a minute later, so it travels only as signed heartbeats over gossip and is never written to the bucket, which would keep a log of when everyone was around. A member who can't reach peers directly shows as "not reachable" rather than "offline", because the app can't tell the two apart. A status is meant to last, so it is an op that syncs like any other and reaches people who were away. Cost: a version older than this one can't read a status op, and because each member's ops apply in order, it holds back everything that member writes afterwards until it updates (it tells the user to update). Statuses set to "As usual" or "None" read fine there; "Mentions only" is the one older versions can't read. Older versions also log a warning for each heartbeat they can't read.
