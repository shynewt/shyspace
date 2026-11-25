---
title: Choose a provider
summary: Where to keep your workspace's encrypted copy, what each provider protects, and what it costs.
---

Shyspace keeps an encrypted copy of your workspace in one storage bucket, so members who were offline can catch up. The provider stores only sealed data: it sees who writes, how much and when, but not what.

Any provider that speaks S3 will work. Where they differ is in protecting your history from the bucket's own keys. Every member's app holds those keys, so with the wrong setup any member could delete the workspace's history for good.

## Our recommendation

Use Scaleway. It's a French company with data centres only in the EU, it charges nothing for requests, and it can keep old versions of your data with a key that can't delete them.

## Compared

| Provider | Company | Your history | 100 GB a month |
|---|---|---|---|
| [Scaleway](scaleway.md) | ✅ French, EU only | ✅ Kept; the key can't delete it | ✅ €1.60, no request fees |
| [Cloudflare R2](cloudflare-r2.md) | 🟡 American, EU option | 🟡 Messages locked; deleted files gone at once | 🟡 $1.50, plus requests |
| [Backblaze B2](backblaze-b2.md) | ❌ American, shares data with advertising networks | 🟡 Kept, but B2 may not work with Shyspace | ✅ $0.70, no request fees |
| [AWS S3](aws-s3.md) | 🟡 American, EU regions | ✅ Kept; the key can't delete it | 🟡 $2.45, plus requests and downloads |
| [MinIO](minio.md) | 🟡 Your own server, no longer maintained | ✅ Kept; the key can't delete it | ✅ Your own hardware |

Prices are the providers' list prices in November 2026, rounded, before tax, for their standard storage in an EU region. Messages take little space; shared files are what make the bill grow.

Shyspace also makes many small requests to the bucket. We haven't yet measured what that costs a typical team where providers charge for requests, so we don't give a monthly total here.

## Each provider in short

Scaleway fits most teams. Stopping its key from deleting old versions takes a bucket policy, which needs some care.

Cloudflare R2 has the strongest lock for your messages and charges nothing for downloads. It can't keep old versions, though, so a deleted or replaced file is gone straight away, and you'd need to back up shared files another way.

Backblaze B2 is the cheapest and always keeps old versions, but we don't recommend it. Its [privacy notice](https://www.backblaze.com/company/policy/privacy) says it shares identifiers with advertising networks, and its documentation doesn't list create-only writes, which Shyspace needs.

AWS S3 is the most complete option, but the most complicated to set up, and downloads cost extra.

MinIO runs on your own server. Its open-source edition is no longer maintained and gets no security fixes, so we don't recommend it for a new setup.

## Other providers

These may work, but we haven't checked enough to give steps:

- Hetzner (Germany) looks promising, but we haven't confirmed that a key can be stopped from deleting old versions.
- At OVHcloud (France), we found no way to stop a key from deleting old versions.
- At IONOS (Germany), that permission can only be set for the whole bucket.
- Wasabi charges for at least 1 TB, and for 90 days of anything deleted.
- Exoscale (Switzerland) has no rules for cleaning up old versions, so you'd keep, and pay for, every deleted file.

With any of them, **Test connection** in Shyspace tells you whether the bucket works, and the admin's **Workspace settings › Storage** tells you whether it keeps deleted data.

## Check the protection

This check tells you whether the members' key really can't destroy history, which the app itself can't see. You need a terminal and the [AWS command-line tool](https://docs.aws.amazon.com/cli/latest/userguide/getting-started-install.html), which works with every provider here.

1. Use the members' key, and your provider's endpoint:

   ```sh
   export AWS_ACCESS_KEY_ID=<access key>
   export AWS_SECRET_ACCESS_KEY=<secret key>
   export AWS_DEFAULT_REGION=<region>
   export AWS_ENDPOINT_URL=<endpoint>
   ```

2. Write a test file, then delete it. Both should work:

   ```sh
   echo check > check.txt
   aws s3api put-object --bucket <bucket> --key protection-check.txt --body check.txt
   aws s3api delete-object --bucket <bucket> --key protection-check.txt
   ```

3. List the file's versions. The one you wrote should still be there:

   ```sh
   aws s3api list-object-versions --bucket <bucket> --prefix protection-check.txt
   ```

4. Try to delete that version, using the `VersionId` from step 3. This must fail with `AccessDenied`:

   ```sh
   aws s3api delete-object --bucket <bucket> --key protection-check.txt --version-id <VersionId>
   ```

If step 4 works, the key can destroy history. Go back to your provider's page and check the step that limits the key. On Cloudflare R2 this check doesn't apply, because R2 keeps no versions; there, **Workspace settings › Storage** shows **Protected** once the lock is in place.
