//! Your own devices: the list Settings › Devices shows, and asking the
//! admin's app to remove some of them.

use anyhow::{Result, anyhow, bail, ensure};
use iroh::PublicKey;

use super::Workspace;
use crate::{
    membership::device_change::DeviceChange,
    ops::{Op, now_ms},
    storage::{DEVICE_NOTICE_LIFETIME_MS, MAX_DEVICES_PER_PERSON},
    views::{Change, DeviceInfo, DeviceNotice, DeviceNoticeKind, member_id, parse_member_id},
    workspace::linking_store::LINKING_LIFETIME_MS,
};

const UNREACHABLE: &str = "Couldn't reach your workspace. Try again.";
const FIRST_FROM_LINKED: &str = "Remove this one from that device, or ask your admin.";
const ADMINS_DEVICE: &str = "The admin's own device can't be removed.";
const KEEP_ONE: &str = "Keep at least one of your devices.";
const ALREADY_REMOVING: &str = "Removing already. It finishes when your workspace confirms.";

impl Workspace {
    /// Your devices that are in the workspace, this one first, then those
    /// one of your devices confirmed that the admin's app hasn't added yet.
    pub fn my_devices(&self) -> Result<Vec<DeviceInfo>> {
        let person = self.my_person();
        let me = self.me();
        let mut devices = Vec::new();
        for device in self.my_member_devices()? {
            let linked_devices = self.linked_through(&device)?.iter().map(member_id).collect();
            let state = self.state();
            let store = &state.store;
            devices.push((
                device,
                DeviceInfo {
                    id: member_id(&device),
                    is_this_device: device == me,
                    is_first_device: device == person,
                    platform: store.device_platform(&device)?,
                    linked_ms: store.device_linked_ms(&device)?,
                    is_active_now: device == me,
                    last_active_ms: store.newest_message_ms(&device)?,
                    has_no_activity_seen: device != me && store.last_seq(&device)? == 0,
                    is_removing: false,
                    is_linking: false,
                    linked_devices,
                    may_remove: false,
                    why_not_removable: None,
                },
            ));
        }
        for (device, linked_ms) in self.my_linking_devices()? {
            // Cancelled, or refused (its voucher then names it too).
            if self.has_asked_to_remove(&device)? {
                continue;
            }
            let platform = self.state().store.device_platform(&device)?;
            devices.push((
                device,
                DeviceInfo {
                    id: member_id(&device),
                    is_this_device: false,
                    is_first_device: false,
                    platform,
                    linked_ms: Some(linked_ms),
                    is_active_now: false,
                    last_active_ms: None,
                    has_no_activity_seen: false,
                    is_removing: false,
                    is_linking: true,
                    linked_devices: Vec::new(),
                    may_remove: false,
                    why_not_removable: None,
                },
            ));
        }
        let now = now_ms()?;
        {
            let presence = self
                .inner
                .presence
                .lock()
                .expect("presence lock poisoned by an earlier panic");
            for (device, info) in devices
                .iter_mut()
                .filter(|(device, info)| *device != me && !info.is_linking)
            {
                let heard_ms = presence.last_heard_ms(device);
                info.is_active_now = presence.is_reachable(device, now);
                info.last_active_ms = info.last_active_ms.max(heard_ms);
                info.has_no_activity_seen &= heard_ms.is_none();
            }
        }
        let epoch = self.state().mls.epoch()?;
        let mine = self.my_member_devices()?;
        for (device, info) in &mut devices {
            info.is_removing = self
                .live_removals_naming(device)?
                .iter()
                .any(|removal| removal.epoch >= epoch);
            let why_not = match info.is_removing {
                true => Some(ALREADY_REMOVING),
                false => self.why_not_removable(device, &mine),
            };
            info.may_remove = why_not.is_none();
            info.why_not_removable = why_not.map(str::to_owned);
        }
        devices.sort_by_key(|(_, info)| !info.is_this_device);
        Ok(devices.into_iter().map(|(_, info)| info).collect())
    }

    /// What happened to your devices in the last 30 days, newest first: one
    /// linked from another of your devices, or removed.
    pub fn device_notices(&self) -> Result<Vec<DeviceNotice>> {
        let store = &self.state().store;
        let since = now_ms()? - DEVICE_NOTICE_LIFETIME_MS;
        store
            .device_notices(since)?
            .into_iter()
            .map(|notice| {
                Ok(DeviceNotice {
                    device: member_id(&notice.device),
                    kind: match notice.kind {
                        crate::storage::DeviceNoticeKind::Linked => DeviceNoticeKind::Linked,
                        crate::storage::DeviceNoticeKind::Removed => DeviceNoticeKind::Removed,
                    },
                    platform: store.device_platform(&notice.device)?,
                    at_ms: notice.at_ms,
                    is_seen: notice.is_seen,
                })
            })
            .collect()
    }

    pub fn mark_device_notices_seen(&self) -> Result<()> {
        self.state().store.mark_device_notices_seen()?;
        self.emit(Change::Members);
        Ok(())
    }

    /// Asks the admin's app to remove one of your devices. Removing this
    /// device goes out to the bucket first, or fails and changes nothing.
    pub async fn remove_my_device(&self, id: &str) -> Result<()> {
        self.ask_to_remove(vec![parse_member_id(id)?]).await
    }

    /// Asks to remove one of your devices and the devices it linked that
    /// are still yours, apart from this one.
    pub async fn remove_my_device_and_its_links(&self, id: &str) -> Result<()> {
        let device = parse_member_id(id)?;
        let mut devices = vec![device];
        devices.extend(self.linked_through(&device)?);
        self.ask_to_remove(devices).await
    }

    async fn ask_to_remove(&self, mut devices: Vec<PublicKey>) -> Result<()> {
        ensure!(!self.is_reading_only(), super::reading::READING_ONLY);
        let mine = self.my_member_devices()?;
        let person = self.my_person();
        let linking: Vec<PublicKey> = self
            .my_linking_devices()?
            .into_iter()
            .map(|(device, _)| device)
            .collect();
        for device in &devices {
            ensure!(
                mine.contains(device) || linking.contains(device),
                "that isn't one of your devices in this workspace"
            );
            if let Some(why_not) = self.why_not_removable_in_any_set(device, person) {
                bail!(why_not);
            }
        }
        ensure!(mine.iter().any(|device| !devices.contains(device)), KEEP_ONE);
        // Asked again unless this device already asked in this epoch: once the
        // epoch moves and the device is still there, the request was skipped.
        let epoch = self.state().mls.epoch()?;
        let mut already_asked = Vec::new();
        for device in &devices {
            let removals = self.live_removals_naming(device)?;
            if removals
                .iter()
                .any(|removal| removal.author == self.me() && removal.epoch >= epoch)
            {
                already_asked.push(*device);
            }
        }
        devices.retain(|device| !already_asked.contains(device));
        if devices.is_empty() {
            return Ok(());
        }
        ensure!(
            devices.len() <= MAX_DEVICES_PER_PERSON,
            "too many devices at once"
        );
        let is_removing_me = devices.contains(&self.me());
        if is_removing_me {
            self.push().await.map_err(|_| anyhow!(UNREACHABLE))?;
        }
        self.write(Op::DeviceChange {
            change: DeviceChange::Remove { devices },
        })?;
        if is_removing_me {
            self.push().await.map_err(|_| anyhow!(UNREACHABLE))?;
        }
        Ok(())
    }

    /// Why this device may not ask to remove `device` on its own, if it
    /// may not: the rules `ask_to_remove` checks, for one device.
    fn why_not_removable(&self, device: &PublicKey, mine: &[PublicKey]) -> Option<&'static str> {
        if self.is_reading_only() {
            return Some(super::reading::READING_ONLY);
        }
        if let Some(why_not) = self.why_not_removable_in_any_set(device, self.my_person()) {
            return Some(why_not);
        }
        let is_only_member = mine.len() == 1 && mine.contains(device);
        is_only_member.then_some(KEEP_ONE)
    }

    /// The rules for one device that hold whatever else is removed with it.
    fn why_not_removable_in_any_set(&self, device: &PublicKey, person: PublicKey) -> Option<&'static str> {
        if *device == person && self.me() != person {
            return Some(FIRST_FROM_LINKED);
        }
        (*device == self.inner.config.admin).then_some(ADMINS_DEVICE)
    }

    /// The devices `device` linked, and those they linked in turn, that are
    /// yours now: never this device, nor what it linked.
    fn linked_through(&self, device: &PublicKey) -> Result<Vec<PublicKey>> {
        let mine = self.my_member_devices()?;
        let mut found: Vec<PublicKey> = Vec::new();
        let mut to_follow = vec![*device];
        while let Some(voucher) = to_follow.pop() {
            for linked in self.state().store.devices_vouched_by(&voucher)? {
                let is_new = linked != *device && !found.contains(&linked);
                if is_new && mine.contains(&linked) && linked != self.me() {
                    found.push(linked);
                    to_follow.push(linked);
                }
            }
        }
        Ok(found)
    }

    /// Devices one of your devices asked to link in the last week that were
    /// never members, with when the link was written; a link this device
    /// knows was refused is left out. Those the person asked to remove
    /// since are still here, so asking again finds them.
    fn my_linking_devices(&self) -> Result<Vec<(PublicKey, i64)>> {
        let person = self.my_person();
        let since = now_ms()? - LINKING_LIFETIME_MS;
        let state = self.state();
        let refused: Vec<PublicKey> = state
            .store
            .pending_links()?
            .into_iter()
            .filter(|link| link.is_refused)
            .map(|link| link.request.device)
            .collect();
        let mut linking: Vec<(PublicKey, i64)> = Vec::new();
        for change in state.store.device_changes()? {
            let DeviceChange::Link { certificate, .. } = &change.change else {
                continue;
            };
            let device = certificate.device;
            let is_new = !linking.iter().any(|(listed, _)| *listed == device);
            if change.person == person
                && change.shown_ms >= since
                && is_new
                && !refused.contains(&device)
                && !state.store.was_ever_member(&device)?
            {
                linking.push((device, change.shown_ms));
            }
        }
        Ok(linking)
    }

    /// Your devices that are members now, your first device first.
    fn my_member_devices(&self) -> Result<Vec<PublicKey>> {
        let state = self.state();
        let mut members = Vec::new();
        for device in state.store.devices_of(&self.my_person())? {
            if state.mls.is_member(&device)? {
                members.push(device);
            }
        }
        Ok(members)
    }
}
