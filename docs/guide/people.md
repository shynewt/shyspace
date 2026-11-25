---
title: Invite and remove people
summary: How the admin brings people in, lets them in safely, and removes them.
---

Only the workspace's admin can invite, let in and remove people.

## Invite someone

1. Open the menu under the workspace's name and choose **Invite people**.
2. Under **Who is it for?**, type their name. It only helps you tell invites apart.
3. Choose **Create invite**, then **Copy invite code**.
4. Send the code to that person privately.

An invite code works once, within 7 days, and holds no passwords. Someone who has the code can only ask to join; they get nothing until you let them in.

To cancel an invite that hasn't been used, open **Workspace settings › Members › Invites** and revoke it under **Codes waiting to be used**.

## Let someone in

When someone uses an invite, a **Join requests** item appears in your workspace menu, and the request shows in **Workspace settings › Members › Requests**.

1. Ask the person to read you the safety code on their screen, on a call or in person. Someone who intercepted the invite could also read a reply sent the same way.
2. Compare it with the code on your screen.
3. If they match, choose **Codes match, let in**. If they don't, choose **Decline**.

Your app must be open to see the request. Theirs shows **Waiting for the admin…**, and they must keep it open until you decide.

People who join later can read the workspace's history, including what was written before they joined.

## Remove someone

1. Open **Workspace settings › Members**.
2. Next to the person, choose the remove button, then confirm.

They keep what they had already synced, but can't read anything written after: new messages and pages use keys they never get. This can't be undone, and inviting them back on the same computer isn't possible yet ([#277](https://github.com/shynewt/shyspace/issues/277)).

A removed person still holds the bucket's keys, so Shyspace then asks you to make new ones. Follow [Change the storage key](storage/change-the-key.md).

## Leave a workspace

Anyone can leave a workspace on their own computer. Open the menu under the workspace's name and choose **Leave workspace…**. The workspace's messages, pages and files are removed from that computer. To come back, you need a new invite.

You stay on the member list until the admin removes you. An admin who leaves loses the admin role, and only the [recovery kit](recovery-kit.md) brings it back.
