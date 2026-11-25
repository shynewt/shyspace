---
title: Change the storage key
summary: After removing someone, make a new bucket key so they can't reach the bucket any more.
---

A removed member can't read anything new, but they keep the bucket's keys until you replace them. With those keys they could delete data, or see how much and when people write. So after each removal, Shyspace asks the admin for a new key and shows the steps in **Workspace settings › Storage**.

## Before you start

You need to be the admin and able to sign in at your storage provider. The steps take a few minutes, but step 3 waits until every member has opened the app, which can take longer.

## 1. Make a new key at your provider

Give it the same rights as the old one, limited to this bucket. It has to be a brand-new key with its own access key: Shyspace refuses any access key a removed member had, so giving the old key a new secret won't do.

- On Scaleway, generate a new API key for the `shyspace-members` application, as in [Scaleway](scaleway.md#2-make-the-members-key).
- On Cloudflare R2, create a new API token as in [Cloudflare R2](cloudflare-r2.md#2-make-the-members-key).
- On Backblaze B2, make a new key with `b2 key create`, as in [Backblaze B2](backblaze-b2.md#2-make-the-members-key).
- On AWS, in **IAM** › **Users**, open `shyspace-members`, then **Security credentials** › **Create access key**, as in [AWS S3](aws-s3.md#2-make-the-members-key).
- On MinIO, add a new user with the same policy, as in [MinIO](minio.md#2-make-the-members-key).

## 2. Save it in Shyspace

1. In **Workspace settings › Storage**, paste the new **Access key** and **Secret key**, then choose **Save storage settings**. The app tests the key against the bucket first, then sends it, encrypted, to the current members only.
2. Save the new recovery kit the app shows in place of the old one, which holds the old key and stops working in step 4.

## 3. Wait until everyone has moved

The steps list each member as **Moved** or **Not yet**, and a member moves the next time their app syncs. Wait until everyone shows **Moved**. Anyone who hasn't moved when you delete the old key is cut off until another member's app is online: it then hands them the new key on its own. If nobody is online, they wait. If their app says it has been away too long to get the new key, they choose **Leave workspace…** and join again with a new invite, and removing their old membership starts another key change.

## 4. Delete the old key

Delete it at your provider. On AWS, deactivate it first, then delete it. Then choose **I deleted the old key**, and the app checks that the old key is refused.

If an invite stops working after the change, make a new one ([#279](https://github.com/shynewt/shyspace/issues/279)).

If you decide not to change the key, choose **Don't change the key**. The removed member's key may then keep working.
