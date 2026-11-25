---
title: Cloudflare R2
summary: The strongest lock for your messages and free downloads, but deleted files are gone at once. Setup takes about 15 minutes.
---

R2 is Cloudflare's storage. It can lock your workspace's messages so that nobody can delete or replace them, not even with the bucket's keys. But R2 can't keep old versions, so a deleted or replaced file is gone straight away; if your shared files matter, back them up another way. Cloudflare is an American company, and R2 can keep a bucket's data in the EU.

R2 charges per request, after a free monthly amount, and nothing for downloads. Prices are on [Cloudflare's pricing page](https://developers.cloudflare.com/r2/pricing/).

> **Note:** these steps follow Cloudflare's documentation as of November 2026 and haven't been tried end to end yet. Check that **Workspace settings** › **Storage** shows **Protected** at the end.

## Before you start

- A Cloudflare account with two-factor login on: **My Profile** › **Authentication**. The first time you use R2, Cloudflare asks you to add a payment method.
- About 15 minutes. You do steps 1 and 2 before creating the workspace in step 3, and step 4 after.

## 1. Create the bucket

1. In the dashboard, open **R2 object storage**, then **Create bucket**.
2. Type a name for the bucket.
3. To keep the data in the EU, choose the **EU** jurisdiction. This can't be changed later.
4. Choose **Create bucket**. New buckets are private.

## 2. Make the members' key

1. In **R2 object storage** › **Overview**, choose **Manage** next to **API Tokens**.
2. Choose **Create Account API token**.
3. Give it **Object Read & Write**, and choose **Apply to specific buckets only** with your bucket.
4. Choose **Create API Token**, then copy the **Access Key ID** and **Secret Access Key**. Cloudflare shows the secret only once.

## 3. Create the workspace

In Shyspace, create the workspace as in [Get started](../get-started.md#3-create-the-workspace), with:

- **Endpoint:** `https://<ACCOUNT_ID>.r2.cloudflarestorage.com`, or `https://<ACCOUNT_ID>.eu.r2.cloudflarestorage.com` for an EU bucket. Your account ID is in the R2 overview.
- **Bucket:** the bucket's name
- **Region:** `auto`
- **Access key** and **Secret key:** from step 2

## 4. Lock the messages

1. In Shyspace, open **Workspace settings** › **Storage** and expand **How to protect your bucket**. It names the workspace's folder: a long ID followed by `/`.
2. At Cloudflare, open the bucket's **Settings** › **Bucket lock rules** › **Add rule**.
3. Name the rule, set the prefix to the folder followed by `ops/`, for example `3f9c2a…/ops/`, and keep objects indefinitely.
4. Choose **Save changes**.

Lock only `ops/`. Don't lock `mls/`: Shyspace repairs it by overwriting. And never lock the whole bucket: Shyspace deletes and replaces some of its own files, such as invites and deleted files.

To delete the workspace's data one day, remove this rule first.

## 5. Check it

In Shyspace, choose **Check again** in **Workspace settings** › **Storage**. It should show **Protected**. This hasn't been tried on a real bucket yet.

## Change the key later

After you remove someone, create a new API token as in step 2; don't roll the old token's secret, because Shyspace refuses a key a removed member had. Then follow [Change the storage key](change-the-key.md), and delete the old token in the same list when the app asks you to.
