---
title: Scaleway
summary: Our recommended provider, a French company with EU data centres and no request fees. Setup takes about 20 minutes.
---

Scaleway is a French company whose storage regions are all in the EU: Paris, Amsterdam, Warsaw and Milan. It charges nothing for requests, and it can keep old versions of your data with a key that can't delete them. Prices are on [Scaleway's pricing page](https://www.scaleway.com/en/pricing/storage/).

> **Note:** these steps follow Scaleway's documentation as of November 2026. Steps 3 and 5 haven't been tried end to end yet, so do the [protection check](choose-a-provider.md#check-the-protection) at the end.

## Before you start

- A Scaleway account with two-factor login on: your avatar › **Settings** › **One-time password**.
- A Project used only for this bucket. The key's rights cover every bucket in its Project.
- About 20 minutes. You do steps 1 to 3 before creating the workspace in step 4, and step 5 after.

## 1. Create the bucket

1. In the side menu, open **Object Storage**, then **Create bucket**.
2. Choose a region: Paris (`fr-par`), Amsterdam (`nl-ams`), Warsaw (`pl-waw`) or Milan (`it-mil`).
3. Type a name for the bucket and choose **Private**.
4. Tick **Enable bucket versioning**. Leave **Enable object lock** off: a lock on the whole bucket stops Shyspace working.
5. Choose **Create bucket**.

Versioning means a deleted or replaced object is kept as an old version. Once it's on, it can't be turned off, only paused.

## 2. Make the members' key

1. Open **IAM & API keys**, then **Applications** › **Create application**. Name it `shyspace-members`.
2. Open **Policies** › **Create policy**. Choose the application, set the scope to **Specific projects** with the bucket's Project, and add these permission sets: `ObjectStorageObjectsRead`, `ObjectStorageObjectsWrite` and `ObjectStorageObjectsDelete`. A new policy can take up to 5 minutes to apply.
3. Open **API keys** › **Generate API key**. Choose the application as the bearer, set the expiration to **Never**, and answer **Yes, set up preferred Project** with the bucket's Project.
4. Copy the **access key** and the **secret key**. Scaleway shows the secret key only once.

## 3. Stop the key from deleting old versions

The delete permission set also lets the key delete old versions. A bucket policy narrows it: once a bucket has one, the key can only do what both its IAM policy and the bucket policy allow.

1. In **Object Storage**, open **Bucket policies** › **Create bucket policy** › **JSON editor**. Keep **Maintain access to bucket** ticked.
2. Paste the policy below and fill in three values:
   - `<BUCKET>`: the bucket's name.
   - `<APPLICATION_ID>`: the ID of `shyspace-members`, shown in **IAM & API keys** › **Applications**.
   - `<YOUR_USER_ID>`: your own user ID, shown in **IAM & API keys** › **Users**.

   ```json
   {
     "Version": "2023-04-17",
     "Id": "ShyspaceMembers",
     "Statement": [
       {
         "Sid": "Admin",
         "Effect": "Allow",
         "Principal": { "SCW": "user_id:<YOUR_USER_ID>" },
         "Action": "s3:*",
         "Resource": ["<BUCKET>", "<BUCKET>/*"]
       },
       {
         "Sid": "MembersList",
         "Effect": "Allow",
         "Principal": { "SCW": "application_id:<APPLICATION_ID>" },
         "Action": ["s3:ListBucket", "s3:ListBucketVersions"],
         "Resource": "<BUCKET>"
       },
       {
         "Sid": "MembersObjects",
         "Effect": "Allow",
         "Principal": { "SCW": "application_id:<APPLICATION_ID>" },
         "Action": ["s3:GetObject", "s3:GetObjectVersion", "s3:PutObject", "s3:DeleteObject"],
         "Resource": "<BUCKET>/*"
       }
     ]
   }
   ```

3. Save the policy.

The policy lets the key read old versions, which Shyspace's protection check needs, but leaves out `s3:DeleteObjectVersion`, so the key can hide an object but not destroy its old versions. Keep the **Admin** part: anyone the policy doesn't name loses access to the bucket. Only the Organization Owner can always change a bucket's policy.

## 4. Create the workspace

In Shyspace, create the workspace as in [Get started](../get-started.md#3-create-the-workspace), with:

- **Endpoint:** `https://s3.<region>.scw.cloud`, for example `https://s3.fr-par.scw.cloud`
- **Bucket:** the bucket's name
- **Region:** `fr-par`, `nl-ams`, `pl-waw` or `it-mil`
- **Access key** and **Secret key:** from step 2

## 5. Let deleted files go after 30 days

These rules let the bucket drop old versions of shared files and of the admin's recovery snapshot after 30 days. Without them, every deleted or replaced file stays in the bucket, and on your bill. Messages and pages keep their whole history. The app's small bookkeeping files keep their old versions as well, but they take little room.

1. In Shyspace, open **Workspace settings** › **Storage** and expand **How to protect your bucket**. It names the workspace's folder: a long ID followed by `/`.
2. At Scaleway, open the bucket's **Lifecycle rules** tab › **Create Lifecycle rule**.
3. Name the rule, filter by **Prefix**, and type the folder followed by `files/`, for example `3f9c2a…/files/`.
4. Choose to expire and delete non-current versions after **30** days, then create the rule.
5. Make a second rule the same way for the folder followed by `recovery/`.

Never make a rule for the whole bucket, or for the `ops/` and `mls/` folders: they hold the history.

## 6. Check it

**Workspace settings** › **Storage** should show **Partly protected** after its next check: the bucket keeps old versions. This hasn't been tried on a real bucket yet ([#278](https://github.com/shynewt/shyspace/issues/278)). Then do the [protection check](choose-a-provider.md#check-the-protection) once, to be sure the key can't delete them.

## Change the key later

After you remove someone, make a new key: in **IAM & API keys** › **API keys**, generate a new key for `shyspace-members` as in step 2. Then follow [Change the storage key](change-the-key.md), and delete the old key on the same page when the app asks you to.
