---
title: Known limits
summary: What Shyspace can't do yet, so you know before your team relies on it.
---

## Devices

- Shyspace runs on computers only: macOS (Apple silicon), Windows and Linux. Phone apps are planned ([#19](https://github.com/shynewt/shyspace/issues/19)).
- Each person can use one computer per workspace. A second computer for the same membership isn't possible yet ([#53](https://github.com/shynewt/shyspace/issues/53)).
- A member who loses their computer has to join again as a new member, because only the admin has a [recovery kit](recovery-kit.md) ([#54](https://github.com/shynewt/shyspace/issues/54)).

## The admin

- A workspace has one admin, and only the admin can invite, let in and remove people, or change the storage settings ([#5](https://github.com/shynewt/shyspace/issues/5)).
- The admin role lives on the admin's computer. If that computer is lost, the [recovery kit](recovery-kit.md) brings the role back; without the kit, nobody can add or remove members any more.

## Storage

- Every member's app holds the bucket's keys, so a member could delete data the bucket doesn't keep. Set the bucket up as your [provider's page](storage/choose-a-provider.md) shows.
- A removed member keeps the bucket's keys until you [change the storage key](storage/change-the-key.md). The app asks you to after each removal.
- The storage provider sees who writes, how much and when, and the name someone types when asking to join ([#276](https://github.com/shynewt/shyspace/issues/276)). It can't see what anyone writes.

## Features

- Huddles are voice only, with up to 8 people: no video or screen sharing. Voice is encrypted between devices but not yet with the channel's own key ([#275](https://github.com/shynewt/shyspace/issues/275)).
- A deleted message stays in the copies of people who had already synced it, so deleting can't take back a secret.
- There is no single sign-on, no compliance export and no admin access to private channels, which larger companies often require.
