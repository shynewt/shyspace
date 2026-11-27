//! Who belongs to the workspace and how keys reach them. MLS (OpenMLS) agrees
//! on one secret per epoch; only the admin changes membership.

pub mod admin;
pub(crate) mod commit;
pub mod device;
pub(crate) mod follow;
pub mod invite;
pub(crate) mod keyring;
pub mod link;
pub mod mls;
pub mod recovery;
pub(crate) mod request_seal;
