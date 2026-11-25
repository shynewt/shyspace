---
title: Recovery kit
summary: What the admin's recovery kit is for, how to keep it, and how to use it on a new computer.
---

Only the admin can add and remove members, and the admin role lives on the admin's computer. If that computer is lost, broken or stolen, the recovery kit brings the role back on a new one. Without the kit, nobody can add or remove members in that workspace any more.

## Save it

1. Open the menu under the workspace's name, then **Workspace settings**.
2. Choose **Recovery**, then **Show recovery kit**.
3. Copy the kit, save it in a password manager, then choose **I saved it**.

The kit is a long code that starts with `shyrec1_`. It holds the key to the admin role and the bucket's keys, so treat it like a password.

## Save it again after changing the storage key

After you [change the storage key](storage/change-the-key.md), Shyspace shows you a new kit with the new key in it. Replace the old kit in your password manager: it stops working once the old key is deleted.

## Use it on a new computer

Only do this if the old computer is gone. Two admins at once would make conflicting changes.

1. Install Shyspace on the new computer.
2. On **Welcome to shyspace**, choose **Restore admin**.
3. Paste the kit and choose **Restore admin**.

The app finds the workspace in the bucket and syncs it from there.

## If a member loses their computer

Members have no recovery kit yet. Remove their old membership in **Workspace settings › Members**, then invite them again. As a new member they can read the workspace's history again, but things only they could see, such as their drafts and saved items, are gone. They also lose their direct messages, whose member list can't change, and their private channels until the channel's owner adds them again.

## A size limit for large teams

The kit restores a snapshot that grows with the number of members. If it nears its limit, **Recovery** shows a warning, and past the limit it says the kit can no longer restore the workspace. Most teams are years away from this.
