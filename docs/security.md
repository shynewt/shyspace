# Security

What shyspace protects, how, and where it falls short. Update this whenever a change touches crypto, membership, sync or what the bucket stores.

## Who we defend against

| Who | Can | Cannot |
|---|---|---|
| The bucket provider, or anyone who reads the bucket | see metadata (who writes, how much, when) | read or forge content |
| Anyone with the bucket credentials (every member) | delete data, disrupt sync | read or forge content, join without the admin |
| Anyone holding an invite code | ask to join once, before it expires | get in without the admin's approval, touch anything else in the bucket |
| A removed member | read what they already had | read anything written after removal |
| A current member | write as themselves | write as someone else, take over others' messages, change membership |
| Someone who copies a member's data directory | nothing, without the device key | read messages, keys or credentials |

## Defenses, and where they live

- **Content encryption:** sign-then-encrypt with XChaCha20-Poly1305. The signature and the AEAD both cover the header (author, seq, epoch). See `crypto.rs`.
- **Per-epoch keys** come from the MLS exporter. A removal starts a new epoch. See `mls.rs`.
- **Author must belong to the op's epoch:** checked in `sync.rs` (`verify`) against `epoch_members`.
- **Gap-free logs:** an author's op applies only right after their previous one, so a log has no holes to fill later. A verified op that breaks a rule (someone else's id, an admin-only change, a damaged page update) is recorded as handled and skipped, so it can't block what follows. Other failures, like a full disk, are retried instead. See `sync/receive.rs`.
- **Removed members' logs are frozen:** before removing someone, the admin pulls their latest ops and puts their last sequence number in the signed removal commit. Every member (and every later newcomer, through the key history) rejects ops from them past that number. See `membership/admin.rs`, `sync/receive.rs`.
- **Owned ids:** a channel, message or page id starts with its creator's key prefix (16 bytes), so nobody else can claim it. See `ops.rs` and `store.rs`.
- **Clock bound:** ops whose lamport clock jumps more than 2^32 ahead wait instead of applying, so one op can't pin titles forever.
- **Junk can't shadow real ops:** ops are stored only after they verify. Parked ops get one row each, keyed by hash, capped per author and in total.
- **Commits are admin-signed;** members ignore anything else. The admin saves a commit before applying it. Every member keeps the commits it applied and re-uploads any that go missing or change in the bucket.
- **Invites:** single-use, expire after 7 days, need the admin's approval, and carry only presigned links (no credentials). The joiner sees a 20-digit safety code derived from their key; the admin sees the requester's code and should compare it out of band before approving, since a leaked code lets someone else ask in the joiner's name. Credentials reach the newcomer only inside the MLS-encrypted answer. See `membership/admin.rs`.
- **Identity binding:** a joiner's key package must be signed by the identity it claims. The invite pins the admin's MLS key, so a forged Welcome is rejected.
- **Members-only gossip:** peers that aren't current MLS members are refused at connection time, and we only dial members. See `transport/gossip.rs`.
- **Admin recovery:** the recovery kit is a secret (recovery key plus bucket credentials). Snapshots in the bucket are encrypted with a key derived from the recovery key and bound to the workspace id. A snapshot is always uploaded before the commits it covers, and restore refuses a snapshot when the bucket holds any admin-signed commit from the snapshot's epoch onward (a rolled-back or stale snapshot). Deleting those slots doesn't help: members re-upload them. A failed restore removes only what it created. See `membership/recovery.rs`.
- **Storage settings:** only the admin can change the bucket region and keys. New settings are checked against the bucket first, travel as an admin-only op encrypted with the current epoch key (so removed members can't read them), and ops claiming to change settings from anyone else are skipped. Newcomers get the settings with their version, so older changes replayed later can't put back revoked keys. The recovery kit holds the keys too, so after a change the app shows the admin a new kit; older kits stop working once the old key is deleted. Plain `http://` endpoints are refused unless the server is on the local network. See `workspace/storage.rs`.
- **Encryption at rest:** both local databases use SQLCipher with keys derived from a device key kept in the OS keychain (or a key file the user places elsewhere). The identity key and bucket credentials are inside the encrypted database; `workspace.json` holds only public settings. See `keys.rs`, `storage/mod.rs`.
- **Chat rendering:** message HTML is sanitized with DOMPurify; only http(s), mailto and `shy://page/` links are allowed. External links open in the system browser.

## Known gaps

- Bucket credentials are shared by all members, so any member can cause denial of service. After removing someone, the admin should rotate the keys: create a new key at the provider, enter it under **Members → Storage** (it reaches current members only), and delete the old key once everyone has synced.
- Single admin. A recovery kit restores a lost admin device, but there is still only one admin at a time; restoring while the old device is alive would cause conflicting membership changes (#5).

Issues labeled `security` track these: <https://github.com/shynewt/shyspace/issues?q=label%3Asecurity>

- A restored admin can reuse sequence numbers of ops that reached peers only live before the old device died (#26).
- A removed member's open gossip connection stays up until it drops; they see traffic timing, not content (#27).

## History

The core was reviewed three times by an independent agent before the first commit. Every finding was fixed or listed above.
