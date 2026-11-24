//! How envelopes travel: live over iroh gossip, durably through an S3 bucket,
//! and, for invitees without credentials, through presigned links.

pub mod blobs;
pub mod bucket;
pub mod direct;
pub mod gossip;
pub mod heads;
pub mod page_gossip;
pub mod presigned;
