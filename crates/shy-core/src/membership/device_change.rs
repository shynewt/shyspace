//! What a person's device asks the admin's app to do about the person's
//! devices: add one it vouched for after a link, or remove some. Every
//! member judges a change as of the epoch it was sealed in
//! (`storage/device_changes.rs`); only the admin's app acts on one.

use anyhow::{Result, ensure};
use iroh::PublicKey;
use serde::{Deserialize, Serialize};

use super::{
    device::DeviceCertificate,
    link::{LinkedDevice, Token},
};
use crate::storage::MAX_DEVICES_PER_PERSON;

/// Far above any key package OpenMLS makes, far below the op limit.
pub const MAX_KEY_PACKAGE_BYTES: usize = 8 * 1024;
const SIGNATURE_BYTES: usize = 64;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DeviceChange {
    /// Add the device the author vouched for, from the link code `token`.
    Link {
        token: Token,
        linked: LinkedDevice,
        certificate: Box<DeviceCertificate>,
    },
    /// Remove these devices of the author's person: one, or one with the
    /// devices linked through it.
    Remove { devices: Vec<PublicKey> },
}

impl DeviceChange {
    /// Checks what can be checked without the store: sizes, counts, and that
    /// a link's parts belong together.
    pub fn check_shape(&self) -> Result<()> {
        match self {
            Self::Link {
                linked, certificate, ..
            } => {
                ensure!(
                    linked.key_package.len() <= MAX_KEY_PACKAGE_BYTES,
                    "a link with an oversized key package"
                );
                ensure!(
                    linked.binding.len() == SIGNATURE_BYTES,
                    "a link with a malformed binding"
                );
                ensure!(
                    linked.acceptance == certificate.device_signature,
                    "a link whose acceptance isn't its certificate's"
                );
            }
            Self::Remove { devices } => {
                ensure!(
                    (1..=MAX_DEVICES_PER_PERSON).contains(&devices.len()),
                    "a removal of no devices or more than a person can have"
                );
                let mut distinct = devices.clone();
                distinct.sort_unstable_by_key(|device| *device.as_bytes());
                distinct.dedup();
                ensure!(distinct.len() == devices.len(), "a removal naming a device twice");
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use iroh::SecretKey;

    use super::*;

    fn link() -> DeviceChange {
        let certificate = DeviceCertificate {
            person: SecretKey::generate().public(),
            device: SecretKey::generate().public(),
            voucher: SecretKey::generate().public(),
            epoch: 2,
            voucher_signature: vec![1; 64],
            device_signature: vec![2; 64],
        };
        DeviceChange::Link {
            token: [3; 16],
            linked: LinkedDevice {
                key_package: vec![4; 300],
                binding: vec![5; 64],
                acceptance: vec![2; 64],
            },
            certificate: Box::new(certificate),
        }
    }

    fn with_linked(change: impl FnOnce(&mut LinkedDevice)) -> DeviceChange {
        let mut link = link();
        if let DeviceChange::Link { linked, .. } = &mut link {
            change(linked);
        }
        link
    }

    #[test]
    fn a_link_must_be_small_and_its_parts_must_belong_together() {
        assert!(link().check_shape().is_ok());
        for misshapen in [
            with_linked(|linked| linked.key_package = vec![0; MAX_KEY_PACKAGE_BYTES + 1]),
            with_linked(|linked| linked.binding = vec![5; 63]),
            with_linked(|linked| linked.acceptance = vec![9; 64]),
        ] {
            assert!(misshapen.check_shape().is_err());
        }
    }

    #[test]
    fn a_removal_names_one_to_five_distinct_devices() {
        let device = || SecretKey::generate().public();
        let one = device();
        let removal = |devices: Vec<PublicKey>| DeviceChange::Remove { devices };

        assert!(removal(vec![one]).check_shape().is_ok());
        assert!(
            removal((0..MAX_DEVICES_PER_PERSON).map(|_| device()).collect())
                .check_shape()
                .is_ok()
        );
        for misshapen in [
            removal(Vec::new()),
            removal((0..=MAX_DEVICES_PER_PERSON).map(|_| device()).collect()),
            removal(vec![one, one]),
        ] {
            assert!(misshapen.check_shape().is_err());
        }
    }
}
