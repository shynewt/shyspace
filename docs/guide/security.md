---
title: Security in plain English
summary: What Shyspace protects, who can see what, and what it doesn't protect against.
---

For the technical details and every known gap, see [the full security model](../security.md).

## What is protected

Everything you write is encrypted on your computer before it leaves it: messages, pages, files, reactions and profile details. The one exception is the name someone types when asking to join, which the bucket can see ([#276](https://github.com/shynewt/shyspace/issues/276)). Only the workspace's members have the keys, which their apps agree on with [MLS](https://www.rfc-editor.org/rfc/rfc9420), the internet standard for encrypted group messaging.

Every change is signed by the person who made it, and every member's app checks the signature, so nobody can write as someone else. Removing someone changes the keys, and the removed person never gets the new ones. Private channels and direct messages have keys of their own, so members who aren't in them can't read them, the admin included.

Your computer keeps its copy encrypted too. The key is in your system's keychain, so someone who copies the app's files can't read them. And the app only installs updates signed with the project's key.

## Who can see what

| Who | Can see | Can't see |
|---|---|---|
| Members of the workspace | Everything in public channels and pages, and everyone's profile | Private channels and direct messages they aren't in |
| The storage provider | Who writes, how much and when, and the name someone types when asking to join ([#276](https://github.com/shynewt/shyspace/issues/276)) | What anyone writes |
| Anyone with the bucket's keys (every member) | The same as the provider | What anyone writes, though they can delete data the bucket doesn't keep |
| Someone who saw an invite code | Nothing. They can only ask to join, and the admin decides | Anything in the workspace |
| A removed member | What they had already synced | Anything written after they left |
| The relay and discovery servers devices connect through (run by n0, the makers of iroh) | A device's IP address, when it's online, and which members' devices it talks to (by their public IDs) and how much | Anything you write |
| GitHub, where updates are published | When the app checks for an update, and from which IP address | Anything in your workspace |

We, the makers of Shyspace, run no server, so we never see your workspace.

## What it doesn't protect against

- A member can copy or share what they read. Encryption only keeps people outside the workspace out.
- Malware on a member's computer can read whatever that member reads.
- Any member could delete the workspace's data, because every member's app holds the bucket's keys. Set the bucket up to keep deleted data, as each provider's page shows. The admin can see how well it's protected in **Workspace settings › Storage**.
- Deleting a message removes it from everyone's app, but someone who had already synced it could still dig it out of their own copy.
- Desktop notifications show message text to your operating system. You can turn the text off in **Settings › Notifications**.
- Whoever holds the project's update key could ship an update that reads your workspace.
- Huddle voice is encrypted between the two devices, so a relay that passes it on can't hear it. Unlike messages, it isn't sealed with the channel's own key yet ([#275](https://github.com/shynewt/shyspace/issues/275)).

## Should you rely on it?

Shyspace is new, and it hasn't had an outside security audit yet. Its code is public, and [the full security model](../security.md) lists every known weakness. If your safety depends on keeping a conversation secret, also use a tool with a longer track record, such as Signal.
