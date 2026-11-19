# Security

What shyspace protects, how, and where it falls short. Update this whenever a change touches crypto, membership, sync or what the bucket stores.

## Who we defend against

| Who | Can | Cannot |
|---|---|---|
| The bucket provider, or anyone who reads the bucket | see metadata (who writes, how much, when) | read or forge content |
| Anyone with the bucket credentials (every member, every invite holder) | delete data, disrupt sync | read or forge content, join without the admin |
| A removed member | read what they already had | read anything written after removal |
| A current member | write as themselves | write as someone else, take over others' messages, change membership |

## Defenses, and where they live

- **Content encryption:** sign-then-encrypt with XChaCha20-Poly1305. The signature and the AEAD both cover the header (author, seq, epoch). See `crypto.rs`.
- **Per-epoch keys** come from the MLS exporter. A removal starts a new epoch. See `mls.rs`.
- **Author must belong to the op's epoch:** checked in `sync.rs` (`verify`) against `epoch_members`.
- **Owned ids:** a channel, message or page id starts with its creator's key prefix (16 bytes), so nobody else can claim it. See `ops.rs` and `store.rs`.
- **Clock bound:** ops whose lamport clock jumps more than 2^32 ahead wait instead of applying, so one op can't pin titles forever.
- **Junk can't shadow real ops:** ops are stored only after they verify. Parked ops get one row each, keyed by hash, capped per author and in total.
- **Commits are admin-signed;** members ignore anything else. The admin saves a commit before applying it, and repairs the bucket copy if it goes missing or changes.
- **Identity binding:** a joiner's key package must be signed by the identity it claims. The invite pins the admin's MLS key, so a forged Welcome is rejected.
- **Chat rendering:** message HTML is sanitized with DOMPurify; only http(s), mailto and `shy://page/` links are allowed. External links open in the system browser.

## Known gaps

- Anyone with the invite code can join, because the admin's app accepts automatically. The code also contains the bucket credentials.
- A removed member could still forge back-dated ops in epochs they belonged to.
- Bucket credentials are shared, so anyone holding them can cause denial of service.
- Local data (SQLite, keys) is not encrypted at rest.
- Single admin: if their device is lost, membership can no longer change.
- Gossip topic membership is not authenticated (content still is).

Open issues on GitHub track the fixes worth doing.

## History

The core was reviewed three times by an independent agent before the first commit. Every finding was fixed or listed above.
