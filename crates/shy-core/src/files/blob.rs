//! The encrypted form of a file's bytes (a "blob"). Each blob has its own
//! random key and is cut into pieces sealed with XChaCha20-Poly1305 in the
//! STREAM construction, so pieces can't be reordered, dropped or cut short.
//! The reference that opens a blob also holds a BLAKE3 hash of every sealed
//! piece. It travels inside a signed op, so someone who holds the key and the
//! bucket credentials still can't swap a piece under the author's name.

use std::io::Read;

use aead_stream::{NewStream, StreamBE32, StreamPrimitive, aead::Payload};
use anyhow::{Context, Result, anyhow, bail, ensure};
use chacha20poly1305::{KeyInit, XChaCha20Poly1305};
use serde::{Deserialize, Serialize};

pub const CHUNK_SIZE: u32 = 4 * 1024 * 1024;
/// Readers accept other piece sizes in this range, so a later version can
/// change it without breaking older ones.
const MIN_CHUNK_SIZE: u32 = 64 * 1024;
const MAX_CHUNK_SIZE: u32 = 16 * 1024 * 1024;
/// What receivers accept at most, far above what this version sends, so a
/// later one can raise its limit; it bounds what a forged size can make a
/// device do.
pub const MAX_RECEIVED_SIZE: u64 = 4 * 1024 * 1024 * 1024;
const FORMAT: u8 = 1;
const TAG_LEN: u64 = 16;
const ID_BYTES: usize = 16;
const NONCE_LEN: usize = 19;
const AAD_CONTEXT: &[u8] = b"shyspace file piece v1";

pub type ChunkHash = [u8; 32];

/// Everything needed to fetch, check and open a blob.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct BlobRef {
    pub format: u8,
    /// Random, so the bucket can't tell equal files apart.
    pub id: String,
    pub key: [u8; 32],
    pub nonce: [u8; NONCE_LEN],
    pub size: u64,
    pub chunk_size: u32,
    pub chunk_hashes: Vec<ChunkHash>,
}

impl std::fmt::Debug for BlobRef {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("BlobRef")
            .field("id", &self.id)
            .field("size", &self.size)
            .field("chunks", &self.chunk_hashes.len())
            .finish_non_exhaustive()
    }
}

impl BlobRef {
    /// Whether a reference from someone else is well formed: a format this
    /// version reads, a real id, and exactly one hash per piece.
    pub fn check(&self) -> Result<()> {
        ensure!(self.format == FORMAT, "unknown file format {}", self.format);
        ensure!(is_blob_id(&self.id), "malformed file id");
        ensure!(
            (MIN_CHUNK_SIZE..=MAX_CHUNK_SIZE).contains(&self.chunk_size),
            "file piece size out of range"
        );
        ensure!(self.size <= MAX_RECEIVED_SIZE, "file too large");
        ensure!(
            u64::try_from(self.chunk_hashes.len())? == chunk_count(self.size, self.chunk_size),
            "file piece count doesn't match its size"
        );
        Ok(())
    }

    pub fn chunk_count(&self) -> u32 {
        // `check` bounds the count by the hashes an op can hold.
        u32::try_from(self.chunk_hashes.len()).unwrap_or(u32::MAX)
    }

    /// Checks a sealed piece against its signed hash and opens it.
    pub fn open_chunk(&self, index: u32, sealed: &[u8]) -> Result<Vec<u8>> {
        let expected = self
            .chunk_hashes
            .get(usize::try_from(index)?)
            .context("no such file piece")?;
        ensure!(
            blake3::hash(sealed).as_bytes() == expected,
            "a file piece doesn't match what its author signed"
        );
        let is_last = index + 1 == self.chunk_count();
        let plaintext = self
            .stream()
            .decrypt(
                index,
                is_last,
                Payload {
                    msg: sealed,
                    aad: &self.aad(),
                },
            )
            .map_err(|_| anyhow!("a file piece doesn't open"))?;
        ensure!(
            u64::try_from(plaintext.len())? == self.plain_len(index),
            "a file piece has the wrong length"
        );
        Ok(plaintext)
    }

    fn plain_len(&self, index: u32) -> u64 {
        let start = u64::from(index) * u64::from(self.chunk_size);
        self.size.saturating_sub(start).min(u64::from(self.chunk_size))
    }

    fn stream(&self) -> StreamBE32<XChaCha20Poly1305> {
        StreamBE32::from_aead(XChaCha20Poly1305::new(&self.key.into()), &self.nonce.into())
    }

    fn aad(&self) -> Vec<u8> {
        [AAD_CONTEXT, self.id.as_bytes()].concat()
    }
}

/// Encrypts exactly `size` bytes from `source`, handing each sealed piece to
/// `store_chunk` in order (with the new blob's id), and returns the reference
/// that opens them.
pub fn seal(
    source: &mut impl Read,
    size: u64,
    mut store_chunk: impl FnMut(&str, u32, &[u8]) -> Result<()>,
) -> Result<BlobRef> {
    let mut blob = BlobRef {
        format: FORMAT,
        id: hex::encode(rand::random::<[u8; ID_BYTES]>()),
        key: rand::random(),
        nonce: rand::random(),
        size,
        chunk_size: CHUNK_SIZE,
        chunk_hashes: Vec::new(),
    };
    let count = u32::try_from(chunk_count(size, CHUNK_SIZE)).context("file too large")?;
    let stream = blob.stream();
    let aad = blob.aad();
    let mut buffer = Vec::new();
    for index in 0..count {
        buffer.clear();
        let wanted = blob.plain_len(index);
        source
            .by_ref()
            .take(wanted)
            .read_to_end(&mut buffer)
            .context("reading the file")?;
        if u64::try_from(buffer.len())? != wanted {
            bail!("the file changed while it was being read");
        }
        let sealed = stream
            .encrypt(
                index,
                index + 1 == count,
                Payload {
                    msg: &buffer,
                    aad: &aad,
                },
            )
            .map_err(|_| anyhow!("couldn't encrypt a file piece"))?;
        blob.chunk_hashes.push(*blake3::hash(&sealed).as_bytes());
        store_chunk(&blob.id, index, &sealed)?;
    }
    let mut rest = [0u8; 1];
    ensure!(
        source.read(&mut rest).context("reading the file")? == 0,
        "the file changed while it was being read"
    );
    Ok(blob)
}

/// An empty file still has one (empty) piece, so its end is authenticated.
fn chunk_count(size: u64, chunk_size: u32) -> u64 {
    size.div_ceil(u64::from(chunk_size)).max(1)
}

/// The size of a sealed piece, for bounding downloads before they finish.
pub fn sealed_chunk_len(blob: &BlobRef, index: u32) -> u64 {
    blob.plain_len(index) + TAG_LEN
}

pub fn is_blob_id(id: &str) -> bool {
    id.len() == ID_BYTES * 2 && id.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sealed(content: &[u8]) -> (BlobRef, Vec<Vec<u8>>) {
        let mut pieces = Vec::new();
        let blob = seal(&mut &content[..], content.len() as u64, |_, index, piece| {
            assert_eq!(index as usize, pieces.len());
            pieces.push(piece.to_vec());
            Ok(())
        })
        .unwrap();
        (blob, pieces)
    }

    fn opened(blob: &BlobRef, pieces: &[Vec<u8>]) -> Result<Vec<u8>> {
        let mut content = Vec::new();
        for (index, piece) in pieces.iter().enumerate() {
            content.extend(blob.open_chunk(index as u32, piece)?);
        }
        Ok(content)
    }

    fn two_and_a_bit_pieces() -> Vec<u8> {
        (0..CHUNK_SIZE as usize * 2 + 1000)
            .map(|index| (index % 251) as u8)
            .collect()
    }

    #[test]
    fn a_file_opens_to_what_was_sealed() {
        let content = two_and_a_bit_pieces();
        let (blob, pieces) = sealed(&content);
        blob.check().unwrap();
        assert_eq!(pieces.len(), 3);
        assert_eq!(pieces[2].len() as u64, sealed_chunk_len(&blob, 2));
        assert_eq!(opened(&blob, &pieces).unwrap(), content);
    }

    #[test]
    fn an_empty_file_has_one_piece() {
        let (blob, pieces) = sealed(b"");
        blob.check().unwrap();
        assert_eq!(pieces.len(), 1);
        assert_eq!(opened(&blob, &pieces).unwrap(), b"");
    }

    #[test]
    fn equal_files_look_different() {
        let (first, first_pieces) = sealed(b"same");
        let (second, second_pieces) = sealed(b"same");
        assert_ne!(first.id, second.id);
        assert_ne!(first_pieces, second_pieces);
    }

    #[test]
    fn a_changed_piece_is_refused() {
        let (blob, mut pieces) = sealed(b"the plan");
        pieces[0][3] ^= 1;
        assert!(opened(&blob, &pieces).is_err());
    }

    #[test]
    fn a_piece_resealed_with_the_key_is_refused() {
        // Anyone who can read the file holds its key, but not the author's
        // signature over the hashes.
        let (blob, pieces) = sealed(b"pay Alice");
        let forged = blob
            .stream()
            .encrypt(
                0,
                true,
                Payload {
                    msg: b"pay Mallory",
                    aad: &blob.aad(),
                },
            )
            .unwrap();
        assert_ne!(pieces[0], forged);
        assert!(blob.open_chunk(0, &forged).is_err());
    }

    #[test]
    fn pieces_out_of_order_are_refused() {
        let content = two_and_a_bit_pieces();
        let (blob, mut pieces) = sealed(&content);
        pieces.swap(0, 1);
        assert!(opened(&blob, &pieces).is_err());
    }

    #[test]
    fn a_file_cut_short_is_refused() {
        let content = two_and_a_bit_pieces();
        let (mut blob, pieces) = sealed(&content);
        // Claiming the file ends after its second piece: that piece wasn't
        // sealed as the last one, so it doesn't open as the end.
        blob.size = u64::from(CHUNK_SIZE) * 2;
        blob.chunk_hashes.truncate(2);
        blob.check().unwrap();
        assert!(blob.open_chunk(1, &pieces[1]).is_err());
    }

    #[test]
    fn a_reference_must_match_its_pieces() {
        let content = two_and_a_bit_pieces();
        let (blob, _) = sealed(&content);
        let mut too_few = blob.clone();
        too_few.chunk_hashes.pop();
        assert!(too_few.check().is_err());
        let mut odd_size = blob.clone();
        odd_size.chunk_size = 3;
        assert!(odd_size.check().is_err());
        let mut newer = blob.clone();
        newer.format = 2;
        assert!(newer.check().is_err());
        let mut bad_id = blob.clone();
        bad_id.id = "../../ops".into();
        assert!(bad_id.check().is_err());
        let mut huge = blob;
        huge.chunk_size = MAX_CHUNK_SIZE;
        huge.size = MAX_RECEIVED_SIZE + 1;
        huge.chunk_hashes = vec![[0; 32]; huge.size.div_ceil(u64::from(MAX_CHUNK_SIZE)) as usize];
        assert!(huge.check().is_err());
    }

    #[test]
    fn a_file_that_grows_while_sealing_is_refused() {
        let content = b"longer than claimed";
        let result = seal(&mut &content[..], 6, |_, _, _| Ok(()));
        assert!(result.is_err());
    }

    #[test]
    fn the_key_never_shows_in_debug_output() {
        let (blob, _) = sealed(b"secret");
        let printed = format!("{blob:?}");
        assert!(!printed.contains(&format!("{:?}", blob.key)));
        assert!(!printed.contains("key"));
    }
}
