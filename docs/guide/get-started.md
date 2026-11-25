---
title: Get started
summary: Set up a workspace and invite your teammates in under an hour.
---

One person sets up the workspace and becomes the admin. Everyone else joins with an invite.

## Before you start

Shyspace only works on Desktop for now. Android and iOS apps are coming soon. So make sureyou and your team have a compatible device:

- Mac with Apple silicon (M1 or newer)
- 64-bit PC with Windows
- Linux (Ubuntu 24.04, Debian 13, Fedora 40 or newer)

You also need a storage bucket at an S3-compatible provider, such as Scaleway or Cloudflare R2. Shyspace has no server: devices sync with each other directly (p2p), and the bucket keeps an encrypted copy for whoever is offline. If you don't have a bucket yet, [Choose a provider](storage/choose-a-provider.md) helps you pick one and set it up, which takes 15 to 30 minutes.

## 1. Install the app

Download the file for your system from the [downloads page](https://shyspace.app/download).

- On macOS, open the `.dmg` file and drag Shyspace to Applications.
- On Windows, run the `x64-setup.exe` file. The installer isn't signed yet, so Windows may say it protected your PC. Choose **More info**, then **Run anyway**.
- On Linux, use the `.AppImage`, or the `.deb` or `.rpm` package. The packages ask for your password when they update.

From then on the app updates itself.

## 2. Have your bucket's details ready

You need five things from your provider:
- the endpoint (the address of its storage service, such as `https://s3.fr-par.scw.cloud`)
- the bucket's name, its region (such as `fr-par`)
- an access key and that key's secret. 

Each provider's page in this guide says where to find them.

## 3. Create the workspace

1. Open Shyspace. On **Welcome to shyspace**, choose **Create**.
2. Fill in **Workspace** with your team's name, and **Your name** with the name others will see.
3. Under **Storage bucket**, paste the **Endpoint**. You can paste your bucket's own address instead: at Scaleway, AWS, Cloudflare R2, Backblaze B2, Hetzner, DigitalOcean or Wasabi, the app then fills in the bucket, region and provider under the field.
4. Check **Bucket** and **Region**, then paste the **Access key** and **Secret key**.
5. Choose **Test connection**. The app writes a small test file to the bucket, reads it back and deletes it. When it shows **The bucket works**, choose **Create workspace**.

If the test fails, the message under it shows the step that failed, followed by your provider's own error. [Troubleshooting](troubleshooting.md#test-connection-fails) lists the usual causes.

The workspace opens with one channel, #general.

## 4. Save your recovery kit

You're the workspace's only admin. If you lose this computer and have no recovery kit, nobody can add or remove members any more.

1. Open the menu under the workspace's name, then **Workspace settings**.
2. Choose **Recovery**, then **Show recovery kit**.
3. Copy the kit, save it in a password manager, and choose **I saved it**.

The kit also holds the bucket's keys, so treat it like a password. [Recovery kit](recovery-kit.md) explains when and how to use it.

## 5. Check how well your bucket keeps data

Every member's app holds the bucket's keys, so with the wrong bucket settings any member could delete the workspace's history for good. The app checks for this once a day. To see the latest result, open **Workspace settings** and choose **Storage**; **Check again** runs the check now. It shows one of three results:

- **Protected:** the bucket refuses to delete the workspace's data.
- **Partly protected:** the bucket keeps old versions, so a deletion can be undone. A correct Scaleway, Backblaze, AWS or MinIO setup should show this, because the app can't see what the members' key is allowed to do.
- **Your storage can lose data:** follow your provider's page in this guide to fix it.

## 6. Invite your first teammate

1. Open the menu under the workspace's name and choose **Invite people**.
2. Under **Who is it for?**, type their name, then choose **Create invite**.
3. Choose **Copy invite code** and send it to them privately, for example in Signal or via [yopass](https://share.yopass.se/). The code works once, within 7 days, and holds no passwords.

Your teammate installs Shyspace, chooses **Join**, pastes the code, types their name and chooses **Ask to join**. Their app shows a safety code and waits for approval.

4. A **Join requests** item appears in your workspace menu. Open it.
5. Ask your teammate to read you their safety code, on a call or in person. If it matches the code on your screen, choose **Codes match, let in**.

Someone who intercepted the invite code would get a different safety code, so a match means the request is really theirs.

## Check that it works

Write a message in #general. While you're both online, it reaches your teammate within seconds. If one of you is offline, it arrives from the bucket when they're back.

## Next

- [Invite and remove people](people.md)
- [Security in plain English](security.md): what Shyspace protects, and what it doesn't
- [Known limits](limits.md)
