---
title: MinIO
summary: Storage on your own server. Its open-source edition is no longer maintained, so we don't recommend it for a new setup.
---

MinIO lets you run S3 storage on your own server, so the data never leaves it. But its open-source edition is [no longer maintained](https://github.com/minio/minio): since 2026 it gets no updates and no security fixes, and its command-line tool `mc` is archived too. We don't recommend it for a new setup. If you already run MinIO, these are the steps.

> **Note:** these steps follow MinIO's source code and documentation as of November 2026 and haven't been tried end to end yet. Do the [protection check](choose-a-provider.md#check-the-protection) at the end.

## Before you start

- A MinIO server reachable over HTTPS, and the `mc` tool connected to it with an admin key: `mc alias set myminio https://<your server> <admin key> <admin secret>`.
- About 15 minutes.

## 1. Create the bucket and keep old versions

```sh
mc mb myminio/<BUCKET>
mc version enable myminio/<BUCKET>
```

## 2. Make the members' key

Save this as `shyspace-members.json`, with your bucket's name in place of `<BUCKET>`:

```json
{
  "Version": "2012-10-17",
  "Statement": [
    { "Effect": "Allow", "Action": ["s3:ListBucket", "s3:ListBucketVersions"], "Resource": ["arn:aws:s3:::<BUCKET>"] },
    {
      "Effect": "Allow",
      "Action": ["s3:GetObject", "s3:GetObjectVersion", "s3:PutObject", "s3:DeleteObject"],
      "Resource": ["arn:aws:s3:::<BUCKET>/*"]
    },
    { "Effect": "Deny", "Action": ["s3:DeleteObjectVersion"], "Resource": ["arn:aws:s3:::<BUCKET>/*"] }
  ]
}
```

Keep the `Deny` statement. In MinIO, `s3:DeleteObject` alone can delete an old version unless `s3:DeleteObjectVersion` is explicitly denied, so leaving it out of the allowed actions isn't enough.

Then create the policy and a user with it, choosing a new access key and secret key:

```sh
mc admin policy create myminio shyspace-members shyspace-members.json
mc admin user add myminio <ACCESS_KEY> <SECRET_KEY>
mc admin policy attach myminio shyspace-members --user <ACCESS_KEY>
```

## 3. Create the workspace

In Shyspace, create the workspace as in [Get started](../get-started.md#3-create-the-workspace), with your server's address as the **Endpoint**, the bucket's name, your server's region (`us-east-1` unless you set another), and the access key and secret key from step 2.

## 4. Let deleted files go after 30 days

These rules let the bucket drop old versions of shared files and of the admin's recovery snapshot after 30 days. Without them, every deleted or replaced file stays in the bucket, and on your disk. Messages and pages keep their whole history. The app's small bookkeeping files keep their old versions as well, but they take little room.

1. In Shyspace, open **Workspace settings** › **Storage** and expand **How to protect your bucket**. It names the workspace's folder: a long ID followed by `/`.
2. Run this, with the folder in place of `<FOLDER>`:

   ```sh
   mc ilm rule add --prefix "<FOLDER>files/" --noncurrent-expire-days 30 myminio/<BUCKET>
   mc ilm rule add --prefix "<FOLDER>recovery/" --noncurrent-expire-days 30 myminio/<BUCKET>
   ```

Never make a rule for the whole bucket, or for the `ops/` and `mls/` folders: they hold the history.

## 5. Check it

**Workspace settings** › **Storage** should show **Partly protected** after its next check. This hasn't been tried on a real bucket yet ([#278](https://github.com/shynewt/shyspace/issues/278)). Then do the [protection check](choose-a-provider.md#check-the-protection).

## Change the key later

After you remove someone, add a new user with the same policy as in step 2, then follow [Change the storage key](change-the-key.md). When the app asks, remove the old user with `mc admin user remove myminio <old access key>`.
