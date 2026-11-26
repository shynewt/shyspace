//! Files shared in channels and DMs, and profile pictures. A file's bytes
//! live encrypted in the bucket (`blob`); the op that shares it carries the
//! key, so whoever can read that op can open the file. A file in a private
//! channel or DM travels sealed like the rest of its content.

pub mod blob;
pub mod cache;

use serde::{Deserialize, Serialize};

use crate::ops::{EntityId, is_visible_in_name};
use blob::BlobRef;

/// The largest file this app shares. Receivers don't enforce it, so a later
/// version can raise it.
pub const MAX_FILE_BYTES: u64 = 150 * 1024 * 1024;
/// Pictures are fetched by every member without asking, so they stay small.
pub const MAX_PICTURE_BYTES: u64 = 512 * 1024;
const MAX_NAME_CHARS: usize = 255;
const MAX_MEDIA_TYPE_CHARS: usize = 100;
const UNKNOWN_MEDIA_TYPE: &str = "application/octet-stream";

/// A file shared in a place (a channel or DM), usually with a message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SharedFile {
    /// Owned by its author, like a message id; separate from the blob, so a
    /// later version can give a file new contents.
    pub id: EntityId,
    pub place: EntityId,
    pub message: Option<EntityId>,
    pub name: String,
    pub media_type: String,
    pub blob: BlobRef,
}

/// A name from someone else, made safe to show and to save under: no
/// control or invisible characters (like display names) and no path
/// separators.
pub fn clean_file_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .filter(|character| is_visible_in_name(*character))
        .map(|character| match character {
            '/' | '\\' => '_',
            other => other,
        })
        .collect();
    let trimmed = cleaned.trim().trim_start_matches('.').trim();
    if trimmed.is_empty() {
        return "file".to_owned();
    }
    trimmed.chars().take(MAX_NAME_CHARS).collect()
}

/// A media type from someone else, kept only if it looks like one. The app
/// decides how to show or open a file from its first bytes, never from this;
/// it only picks the icon a notification shows.
pub fn clean_media_type(media_type: &str) -> String {
    let is_token = |part: &str| {
        !part.is_empty()
            && part
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'+' | b'-'))
    };
    let lowered = media_type.to_ascii_lowercase();
    match lowered.split_once('/') {
        Some((kind, subtype))
            if lowered.len() <= MAX_MEDIA_TYPE_CHARS && is_token(kind) && is_token(subtype) =>
        {
            lowered
        }
        _ => UNKNOWN_MEDIA_TYPE.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_from_others_are_safe_to_show_and_save() {
        assert_eq!(clean_file_name("report.pdf"), "report.pdf");
        assert_eq!(clean_file_name("../../.ssh/id_ed25519"), "_.._.ssh_id_ed25519");
        assert_eq!(clean_file_name("..\\boot.ini"), "_boot.ini");
        assert_eq!(clean_file_name(".bashrc"), "bashrc");
        assert_eq!(clean_file_name("invoice\u{202e}fdp.exe"), "invoicefdp.exe");
        assert_eq!(clean_file_name("   "), "file");
        assert_eq!(clean_file_name(&"a".repeat(1000)).chars().count(), 255);
    }

    #[test]
    fn media_types_from_others_are_kept_only_when_well_formed() {
        assert_eq!(clean_media_type("image/PNG"), "image/png");
        assert_eq!(
            clean_media_type("application/vnd.ms-excel"),
            "application/vnd.ms-excel"
        );
        assert_eq!(clean_media_type("text/html; charset=utf-8"), UNKNOWN_MEDIA_TYPE);
        assert_eq!(clean_media_type("nonsense"), UNKNOWN_MEDIA_TYPE);
        assert_eq!(clean_media_type(""), UNKNOWN_MEDIA_TYPE);
    }
}
