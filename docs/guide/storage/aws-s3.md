---
title: AWS S3
summary: The most complete option, but the most complicated to set up, and downloads cost extra. Setup takes about 30 minutes.
---

Amazon S3 is the original S3 storage. It can keep old versions with a key that can't delete them, and it has regions in the EU. It's also the most complicated to set up, and it charges for requests and for downloads. Prices are on [AWS's pricing page](https://aws.amazon.com/s3/pricing/).

> **Note:** these steps follow AWS's documentation as of November 2026 and haven't been tried end to end yet. Do the [protection check](choose-a-provider.md#check-the-protection) at the end.

## Before you start

- An AWS account with multi-factor login on: your account name › **Security credentials** › **Assign MFA device**.
- About 30 minutes. You do steps 1 and 2 before creating the workspace in step 3, and step 4 after.

## 1. Create the bucket

1. Choose a region in the top bar, such as Europe (Frankfurt), `eu-central-1`.
2. Open **S3** › **General purpose buckets** › **Create bucket**.
3. Type a **Bucket name**. Keep **Block all public access** on.
4. Under **Bucket Versioning**, choose **Enable**.
5. Choose **Create bucket**.

## 2. Make the members' key

1. Open **IAM** › **Policies** › **Create policy** › **JSON**, and paste this, with your bucket's name in place of `<BUCKET>`:

   ```json
   {
     "Version": "2012-10-17",
     "Statement": [
       {
         "Effect": "Allow",
         "Action": ["s3:ListBucket", "s3:ListBucketVersions"],
         "Resource": "arn:aws:s3:::<BUCKET>"
       },
       {
         "Effect": "Allow",
         "Action": ["s3:GetObject", "s3:GetObjectVersion", "s3:PutObject", "s3:DeleteObject"],
         "Resource": "arn:aws:s3:::<BUCKET>/*"
       }
     ]
   }
   ```

2. Choose **Next**, name the policy `shyspace-members`, and choose **Create policy**.
3. Open **IAM** › **Users** › **Create user**. Name it `shyspace-members` and attach the policy.
4. Open the user, then **Security credentials** › **Create access key**. Choose **Other**, then **Create access key**.
5. Copy the **access key** and the **secret access key**, or download the `.csv` file. AWS shows the secret only once.

The policy lets the key read old versions, which Shyspace's protection check needs, but leaves out `s3:DeleteObjectVersion`. On a versioned bucket a delete then only adds a delete marker, and the old version stays.

## 3. Create the workspace

In Shyspace, create the workspace as in [Get started](../get-started.md#3-create-the-workspace), with:

- **Endpoint:** `https://s3.<region>.amazonaws.com`, for example `https://s3.eu-central-1.amazonaws.com`
- **Bucket:** the bucket's name
- **Region:** for example `eu-central-1`
- **Access key** and **Secret key:** from step 2

## 4. Let deleted files go after 30 days

These rules let the bucket drop old versions of shared files and of the admin's recovery snapshot after 30 days. Without them, every deleted or replaced file stays in the bucket, and on your bill. Messages and pages keep their whole history. The app's small bookkeeping files keep their old versions as well, but they take little room.

1. In Shyspace, open **Workspace settings** › **Storage** and expand **How to protect your bucket**. It names the workspace's folder: a long ID followed by `/`.
2. At AWS, open the bucket's **Management** tab › **Create lifecycle rule**, and name it.
3. Choose **Limit the scope to specific prefixes or tags**, and type the folder followed by `files/` as the **Prefix**, for example `3f9c2a…/files/`.
4. Tick **Permanently delete noncurrent versions of objects**, set **Days after objects become noncurrent** to **30**, and choose **Create rule**.
5. Make a second rule the same way for the folder followed by `recovery/`.

Never make a rule for the whole bucket, or for the `ops/` and `mls/` folders. And don't set an Object Lock default retention: it covers the whole bucket, and Shyspace must be able to delete some of its own files.

## 5. Check it

**Workspace settings** › **Storage** should show **Partly protected** after its next check. This hasn't been tried on a real bucket yet ([#278](https://github.com/shynewt/shyspace/issues/278)). Then do the [protection check](choose-a-provider.md#check-the-protection).

## Change the key later

After you remove someone, open the user's **Security credentials** › **Create access key** as in step 2, then follow [Change the storage key](change-the-key.md). When the app asks, deactivate the old key, then delete it.
