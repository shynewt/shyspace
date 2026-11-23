//! Voice huddles: who is in which channel's huddle, and who may hear whom.
//! This module holds the rules and state and no audio: the media layer (the
//! `shy-media` crate) moves the sound behind the `HuddleMedia` interface.
//! See "Voice huddles" in docs/decisions.md.

pub mod beacon;
pub mod book;
pub mod media;
pub mod ring;
pub mod rules;
