---
title: Troubleshooting
summary: The usual problems when setting up and running a workspace, and how to fix them.
---

## Test connection fails

The message under **Test connection** says which step failed, followed by your storage provider's own message. The usual causes:

- The endpoint is wrong. It's the provider's address without the bucket name, such as `https://s3.fr-par.scw.cloud`; each provider's page gives the exact form.
- The region doesn't match the bucket. On Cloudflare R2 it's always `auto`.
- The bucket name has a typo, or the bucket is in a different project or account from the key.
- The key isn't allowed to use this bucket. At the provider, check that it can read, write, list and delete objects in it.
- Part of the secret key is missing. Most providers show it only once, so if in doubt, make a new key.

## Storage says "Partly protected"

A correct setup at Scaleway, Backblaze, AWS or MinIO should show this. The bucket keeps old versions, but the app can't see whether the members' key may delete them, so do the [protection check](storage/choose-a-provider.md#check-the-protection) once.

## Storage says "Your storage can lose data"

The bucket doesn't keep deleted or overwritten data, so anyone with its keys could destroy your history. Follow your [provider's page](storage/choose-a-provider.md) to turn on versioning or a lock, then choose **Check again**.

## Storage says "Couldn't check old versions"

The bucket keeps versions, but the members' key isn't allowed to read old ones, so the app can't check that they hold your data. Add `s3:GetObjectVersion` to the key's permissions (your [provider's page](storage/choose-a-provider.md) has the full list), then choose **Check again**.

## Storage says "Couldn't check yet"

No check has finished yet, or the last one failed. The app tries again every hour; choose **Check again** to see the error now.

## Someone is stuck on "Waiting for the admin…"

The admin lets people in from their own app, so it has to be open. Ask the admin to open the menu under the workspace's name and choose **Join requests**. If the invite is older than 7 days or was already used, make a new one.

## Someone can't sync after a key change

If the old key was deleted before their app moved to the new one, their app can't reach the bucket. It gets the new key from another member as soon as one is online, without anyone doing anything; until then their app says the storage key on this device no longer works. If their app says it has been away too long to get the new key, they choose **Leave workspace…** on their computer, then join again with a new invite. Their old membership stays listed until you remove it, and removing it starts another key change. Next time, wait until everyone shows **Moved** before you delete the old key.

## "The old key still works"

The old key is still active at the provider, perhaps because a different key was deleted. Delete the old one at the provider, then choose **Check again**.

## Windows says it protected your PC

The Windows installer isn't signed yet. Choose **More info**, then **Run anyway**.

## The app doesn't start on Linux

It needs a recent system: Ubuntu 24.04, Debian 13, Fedora 40 or newer.

## Still stuck?

Search the [open issues](https://github.com/shynewt/shyspace/issues), or open a new one. Never paste invite codes, recovery kits or bucket keys into an issue.
