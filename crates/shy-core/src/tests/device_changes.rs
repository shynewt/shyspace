//! Device changes as every member judges them: who may ask to link or
//! remove which device, and that the result is the same everywhere.

use iroh::SecretKey;

use super::support::*;
use crate::{
    Workspace,
    membership::{device::DeviceCertificate, device_change::DeviceChange, link::LinkedDevice},
    ops::{Envelope, Op},
    storage::RecordedChange,
};

/// One forger per author for a whole test: each seals the author's next op.
fn change(forger: &mut Forger, change: DeviceChange) -> Envelope {
    forger.seal(Op::DeviceChange { change })
}

fn deliver(envelope: &Envelope, members: &[&Workspace]) {
    for member in members {
        // A void change is skipped, which may surface as an error.
        let _ = member.receive(envelope.clone());
    }
}

/// The changes `member` kept, without the time each was shown (a device's own).
fn kept(member: &Workspace) -> Vec<(iroh::PublicKey, u64, iroh::PublicKey, DeviceChange)> {
    member
        .state()
        .store
        .device_changes()
        .unwrap()
        .into_iter()
        .map(
            |RecordedChange {
                 author,
                 seq,
                 person,
                 change,
                 ..
             }| (author, seq, person, change),
        )
        .collect()
}

fn link_of(certificate: DeviceCertificate) -> DeviceChange {
    DeviceChange::Link {
        token: [1; 16],
        linked: LinkedDevice {
            key_package: vec![2; 200],
            binding: vec![3; 64],
            acceptance: certificate.device_signature.clone(),
        },
        certificate: Box::new(certificate),
    }
}

fn removal(devices: &[iroh::PublicKey]) -> DeviceChange {
    DeviceChange::Remove {
        devices: devices.to_vec(),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_persons_device_can_ask_to_link_a_device_it_vouched_for() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    sync_all(&[&admin, &bob, &carol]).await;
    let mut bobs = Forger::new(&bob);
    let new_device = SecretKey::generate();
    let link = link_of(device_certificate(
        &bob.inner.identity,
        &bob.me(),
        &new_device,
        epoch_of(&bob),
    ));

    deliver(&change(&mut bobs, link.clone()), &[&admin, &carol]);

    for member in [&admin, &carol] {
        let kept = kept(member);
        assert_eq!(kept.len(), 1);
        assert_eq!((kept[0].0, kept[0].2), (bob.me(), bob.me()));
        assert_eq!(kept[0].3, link);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_link_not_vouched_for_by_the_persons_own_device_is_void() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    sync_all(&[&admin, &bob, &carol]).await;
    let mut bobs = Forger::new(&bob);
    let epoch = epoch_of(&bob);
    let mut forged = device_certificate(&bob.inner.identity, &bob.me(), &SecretKey::generate(), epoch);
    forged.voucher_signature[0] ^= 1;

    for certificate in [
        // For another person.
        device_certificate(&bob.inner.identity, &carol.me(), &SecretKey::generate(), epoch),
        // Signed by another device.
        device_certificate(&admin.inner.identity, &bob.me(), &SecretKey::generate(), epoch),
        forged,
    ] {
        deliver(&change(&mut bobs, link_of(certificate)), &[&admin, &carol]);
    }

    assert!(kept(&admin).is_empty());
    assert!(kept(&carol).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_removal_may_name_only_the_persons_own_devices() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    let phone = link_device(&admin, &bob, &root.path().join("phone"), &bucket).await;
    sync_all(&[&admin, &bob, &carol, &phone]).await;
    let mut bobs = Forger::new(&bob);
    let mut phones = Forger::new(&phone);

    deliver(&change(&mut bobs, removal(&[carol.me()])), &[&admin]);
    deliver(&change(&mut bobs, removal(&[phone.me(), carol.me()])), &[&admin]);
    assert!(kept(&admin).is_empty());

    deliver(&change(&mut bobs, removal(&[phone.me()])), &[&admin]);
    deliver(&change(&mut phones, removal(&[phone.me()])), &[&admin]);
    let kept = kept(&admin);
    assert_eq!(
        kept.len(),
        2,
        "the first device removing its phone, and the phone itself"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_linked_device_cannot_ask_to_remove_the_first_device() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let phone = link_device(&admin, &bob, &root.path().join("phone"), &bucket).await;
    sync_all(&[&admin, &bob, &phone]).await;
    let mut phones = Forger::new(&phone);

    deliver(&change(&mut phones, removal(&[bob.me()])), &[&admin, &bob]);

    assert!(kept(&admin).is_empty());
    assert!(kept(&bob).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_admins_first_device_cannot_be_removed_by_request() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let admins_phone = link_device(&admin, &admin, &root.path().join("phone"), &bucket).await;
    sync_all(&[&admin, &bob, &admins_phone]).await;
    let mut admins = Forger::new(&admin);

    deliver(&change(&mut admins, removal(&[admin.me()])), &[&bob]);
    deliver(
        &change(&mut admins, removal(&[admin.me(), admins_phone.me()])),
        &[&bob],
    );

    assert!(kept(&bob).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn device_changes_are_the_same_in_any_order_and_applied_twice() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    let phone = link_device(&admin, &bob, &root.path().join("phone"), &bucket).await;
    let dave = join(&admin, &root.path().join("dave"), &bucket, "Dave", false).await;
    sync_all(&[&admin, &bob, &carol, &phone, &dave]).await;
    let mut bobs = Forger::new(&bob);
    let mut phones = Forger::new(&phone);
    let mut carols = Forger::new(&carol);
    let new_device = SecretKey::generate();
    let changes = [
        change(
            &mut bobs,
            link_of(device_certificate(
                &bob.inner.identity,
                &bob.me(),
                &new_device,
                epoch_of(&bob),
            )),
        ),
        change(&mut phones, removal(&[phone.me()])),
        change(&mut carols, removal(&[carol.me()])),
    ];

    for change in &changes {
        deliver(change, &[&admin]);
    }
    for change in changes.iter().rev().chain(&changes) {
        deliver(change, &[&dave]);
    }

    assert_eq!(kept(&admin).len(), 3);
    assert_eq!(kept(&dave), kept(&admin));
    dave.state().store.rebuild_views(dave.apply_context()).unwrap();
    assert_eq!(kept(&dave), kept(&admin), "the same after a rebuild");
}
