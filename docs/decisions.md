# Decisions

Why things are the way they are. Add an entry when you make a choice that someone might later question. Newest first.

## Mobile apps will be native, on the shared Rust core (2026-11)
iOS and Android get native interfaces (SwiftUI and Jetpack Compose) that call `shy-core` through generated bindings (UniFFI), not the desktop's React app in a Tauri mobile web view. A chat app is judged on fast scrolling lists, the keyboard, back-swipe and screen readers, which web views on phones do worst; Element X and Bitwarden made the same move onto a shared Rust core. What's shared is the core and the design rules (tokens, type scale, motion, behaviour); the screens are rewritten, and the page editor may stay an embedded web view. So the desktop layout is responsive for desktop windows only (full screen and split screen), and logic that every app needs belongs in the core, not in TypeScript. Mobile work is #19.

## Profile details: a new op, every field public, cut or cleared (2026-11)
Title, pronouns, bio, contact, time zone and working hours travel in their own op, `ProfileDetails`, not as new fields on `Profile`: ops are postcard, which reads fields by position, so a longer `Profile` would stop every old `Profile` op from decoding. Versions before it park a member's ops from their first details onward until they update, the same cost as every new kind of op. Every filled field is visible to the whole workspace; there are no per-field audiences, which would need per-field keys. Free text from others is cut to its limit, like names; an email, phone, website or time zone that's too long or malformed is cleared, because a cut address can point somewhere else. Pictures wait for files (#13).

## Views switch without an exit animation (2026-11)
A view fades and rises in; the one it replaces goes at once. With an exit animation (Motion's `AnimatePresence mode="wait"`), switching again while the new view was fading in could leave Motion waiting forever on the exit, with nothing shown. Slow frames made that common, and WebKit's tests caught it. A UI test switches views quickly on simulated slow frames.

## UI tests run in WebKit too, in a container locally (2026-11)
The app runs in WebKitGTK, and Chromium-only tests let WebKit bugs through. Playwright's WebKit build needs Ubuntu's libraries, so locally it runs in the official Playwright image with Podman (`pnpm test:ui:webkit`); CI runs both browsers directly on Ubuntu. The WebKit tests claim a WebKitGTK user agent, because Playwright's WebKit poses as Safari on a Mac and the app picks Mac shortcuts from that.

## Emoji data ships with the app (2026-11)
The emoji picker is frimousse, on Emojibase data. By default it downloads that data from a CDN, which would tell a third party when someone opens a picker in a private app. And the app's content policy lets the page connect only to the core, so it can't fetch the data even from itself. So `ui/emojiData.ts` imports `emojibase-data`'s English files as a lazily loaded module and hands them to the picker; the picker itself loads on first open too, so neither weighs on startup. That function is the one place that builds the categories, so a later "Custom" category goes there. A UI test fails if the picker makes any request outside the app, or doesn't work under the app's connect rules. Because the data is ours, frimousse doesn't check which emoji the system font can draw; we offer Emoji 15.1 and older. Updating emoji means updating the package.

## No link previews (2026-11)
The app never fetches a link to show a preview. Fetching would tell the linked site who is reading what, outside the encryption, and previews are a known way to leak data out of AI agents' context (Slack's MCP server did). Links stay plain.

## Replies are threads, built on reply_to (2026-11)
One reply model: a reply lives in a side-panel thread instead of the channel, like Slack, with Zulip's attention tools (follow and unfollow, following threads you join). Threads are optional, never forced like Zulip topics. Thread replies don't add to a channel's unread count, but a mention in one still does, until you've read it in the channel or the thread. No new op was needed; a title and a "resolved" mark would need one (#58).

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

## Over-long text from others is cut, not refused (2026-11)
Messages, edits and page titles from other members are cut to the limit (40,000 and 200 characters) instead of skipped. Devices from before the limit accepted such ops, so refusing them now would make old and new devices disagree about whether a message exists, and would hold up the author's later ops. Cutting by characters is the same on every device. Cost: a cut can split an emoji or a link at the very end, which only looks odd, and old devices still show the whole text until they update.

## Oversized ops are refused, not cut (2026-11)
An op over its size limit (4 MB for a page update, 256 KB otherwise) is recorded as handled without its content, unlike over-long text, which is cut. A Yjs update or a key grant can't be cut and still mean anything, and keeping the bytes is the very cost the limit removes. Older versions still accept such ops: they show a page update over 4 MB, and an oversized message cut to its first 40,000 characters, where new versions show nothing. Honest ops never get that large apart from rare giant pastes made on an old version, and new versions never send one, so in practice only ops built on purpose look different. One limit per kind rather than per field keeps the rule to a single check on the sealed size.

## Forked ops are dropped, not resolved (2026-11)
When an author signs two different versions of one op, neither counts anywhere, and the proof travels as a bucket object rather than an op. Picking a winner (say the lower hash) would let the author rewrite an old message on every device by signing a new version, and each new version would force another rebuild. Freezing the author's log at the fork would end an honest restored admin's log for good (#26). An op carrying the proof would share its reporter's fate (a freeze could cut it on some devices) and older versions would stop applying every reporter's later ops. Hash-chaining each author's log would stop forks at the source but changes every op; we didn't, for now.

## Joining a private channel: one note, after reading (2026-11)
A member counts as joined once an op of theirs, sealed with a key from their latest add on, counts in the channel; a device that has the key and was opened writes one `Joined` op for that (#110). It isn't written as soon as the key arrives: then anyone listing people in a channel, or every rotation, would make many devices write at once, which shows outsiders who is in each channel and can be abused. It isn't per key generation either: everyone would turn "Invited" after each rotation, and each new note would be a read receipt. The cost is a one-time signal to the channel's members that you opened it.

## Private channels: key grants with commitments, one owner (2026-11)
A private channel's content is sealed under its own key inside an ordinary op, so sync and gap-free logs keep working for everyone. Keys reach members wrapped with HPKE to a per-member sealing key; a removal makes a new key for the rest (the scheme of Signal's sender keys and Matrix's Megolm). We didn't use one MLS group per channel: MLS needs one agreed order of membership changes, which only a server or a single always-online committer gives. Instead each channel has one owner who changes membership and vouches for every key with a commitment, so everything merges in any order and nobody can slip in a fake key. People added later read the history, like Slack. Owners can hand over, and when the admin removes an owner from the workspace, the member they added earliest takes over: computed from facts every device ends up knowing (the removal, the owner's log up to it, the order of adds), written as a claim at the next number so it lands in the owner log like any other change. We didn't let the admin pick (they can't see private channels) or let members race to claim (devices would disagree on who was first). DMs are fixed member lists with no owner, and each member writes only with a key they made and granted to every member themselves (#72). Otherwise one member could hand Bob a key Carol never got, and Bob would write to them both unread. We didn't make members confirm each key before it counts: one member offline, on an older version or removed would stall the DM, and it still wouldn't stop a member showing their own messages to only some people. Reading stays open to any of the DM's keys, so older versions still read and are read. Cost: no healing after a device compromise until the next removal or rotation, and versions older than this one stop applying a member's ops once that member announces a sealing key (every member does on first start), until they update.


## Accent colour: six presets per device, not a free picker (2026-11)
Each person picks Violet, Blue, Indigo, Teal, Pink or Graphite for their own app. Hand-tuned presets can each be checked for WCAG AA in light and dark; a free picker would let people choose colours that make text unreadable. Red, orange and green are left out because they already mean danger, warning and success. A workspace brand colour set by the admin would need core changes and is not planned for now.

## Responsive desktop layout: container queries, a drawer, no icon rail (2026-11)
The layout follows the region's own width (CSS container queries) rather than the window's, so a docked thread narrowing the channel and a narrow window behave alike; JS reads the same width tokens only where behaviour changes (docked or sliding over). Below 900 px the sidebar is a modal drawer, and it can be hidden at any width, fully, as in Notion and Linear. We didn't add an icon rail: a channel list as icons is a column of "#". The thread slides over the channel without a scrim when the channel would keep under 560 px, so the channel stays readable at its edge and keeps working. Header labels fold into icons with tooltips, then into one Info popover, so the title is always the part that's cut last.
