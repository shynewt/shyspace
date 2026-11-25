---
title: Backblaze B2
summary: The cheapest option, but not recommended. It may not work with Shyspace, and Backblaze shares data with advertising networks.
---

Backblaze B2 is cheap, has no request fees and always keeps old versions, but we don't recommend it.

Shyspace writes messages with create-only writes (the `If-None-Match` header), so nobody can replace one. Backblaze's documentation doesn't list them, and one report says B2 refuses them. If it does, **Test connection** fails with "the bucket refuses create-only writes", and B2 can't be used.

Backblaze's [privacy notice](https://www.backblaze.com/company/policy/privacy) also says it has shared identifiers and network activity with advertising networks and analytics providers.

If you still want to try it, these are the steps. Prices are on [Backblaze's pricing page](https://www.backblaze.com/cloud-storage/pricing).

> **Note:** these steps follow Backblaze's documentation as of November 2026 and haven't been tried end to end yet. Try **Test connection** (step 4) before anything else, then do the [protection check](choose-a-provider.md#check-the-protection) at the end.

## Before you start

- A Backblaze account. You choose your account's data region when you sign up, and you can't change it later. Pick **EU Central** (Amsterdam) to keep data in the EU.
- Backblaze's command-line tool, [`b2`](https://github.com/Backblaze/B2_Command_Line_Tool). The website can't make a key with the limits Shyspace needs.
- About 20 minutes.

## 1. Create the bucket

1. Open **B2 Cloud Storage** › **Buckets** › **Create a Bucket**.
2. Type a name and choose **Private**. Leave **Object Lock** off.
3. Choose **Create a Bucket**, and copy the bucket's **Endpoint**, such as `s3.eu-central-003.backblazeb2.com`.

B2 keeps all old versions by default, so there's nothing to turn on.

## 2. Make the members' key

A key made on the website with **Read and Write** can also delete old versions. Make one without that right in a terminal:

```sh
b2 account authorize
b2 key create --bucket <BUCKET> shyspace-members listBuckets,listFiles,readFiles,writeFiles
```

`b2 account authorize` asks for your account's master key. The second command prints the new key's ID, then its secret, once. Copy both.

Without `deleteFiles`, a delete only hides a file and keeps its old version.

## 3. Create the workspace

In Shyspace, create the workspace as in [Get started](../get-started.md#3-create-the-workspace), with:

- **Endpoint:** `https://` followed by the bucket's endpoint, for example `https://s3.eu-central-003.backblazeb2.com`
- **Bucket:** the bucket's name
- **Region:** the middle of the endpoint, for example `eu-central-003`
- **Access key:** the key's ID, and **Secret key:** its secret

## 4. Test the connection first

Choose **Test connection** before anything else. If it fails with "the bucket refuses create-only writes", B2 doesn't work with Shyspace: choose another provider.

## 5. Let deleted files go after 30 days

These rules let the bucket drop old versions of shared files and of the admin's recovery snapshot after 30 days. Without them, every deleted or replaced file stays in the bucket, and on your bill. Messages and pages keep their whole history. The app's small bookkeeping files keep their old versions as well, but they take little room.

1. In Shyspace, open **Workspace settings** › **Storage** and expand **How to protect your bucket**. It names the workspace's folder: a long ID followed by `/`.
2. Run this, with the folder in place of `<FOLDER>`, such as `3f9c2a…/`. It replaces any lifecycle rules the bucket already has.

   ```sh
   b2 bucket update \
     --lifecycle-rule '{"fileNamePrefix": "<FOLDER>files/", "daysFromHidingToDeleting": 30, "daysFromUploadingToHiding": null}' \
     --lifecycle-rule '{"fileNamePrefix": "<FOLDER>recovery/", "daysFromHidingToDeleting": 30, "daysFromUploadingToHiding": null}' \
     <BUCKET>
   ```

## 6. Check it

**Workspace settings** › **Storage** should show **Partly protected** after its next check. This hasn't been tried on a real bucket yet. Then do the [protection check](choose-a-provider.md#check-the-protection).

## Change the key later

After you remove someone, make a new key with `b2 key create` as in step 2, then follow [Change the storage key](change-the-key.md). When the app asks, delete the old key with `b2 key delete <old key ID>`, or **Delete Key** in **Application Keys** on the website.
