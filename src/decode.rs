//! The outer structure: blocks, groups and chunks.

use alloc::vec;
use alloc::vec::Vec;
use core::cmp::Ordering;

use crate::array;
use crate::error::{Error, Result};
use crate::lz;

/// Decoded bytes per block; each block has a 2-byte header.
const BLOCK: usize = 0x4_0000;
/// Decoded bytes per chunk; a block holds up to two.
const CHUNK: usize = 0x2_0000;

/// Stream type identifiers this crate knows from the second header byte.
/// Only [`TYPE_BYTEWISE`] is decoded; the others are recognised so that their
/// stored (uncompressed) blocks can still be read.
const KNOWN_TYPES: [u8; 5] = [5, 6, 10, 11, 12];
/// The byte-oriented stream type with entropy-coded parts.
const TYPE_BYTEWISE: u8 = 10;

/// Decodes `src` into `dst`.
///
/// `dst` must be exactly as long as the decoded data: the format does not
/// record that length, so the container around it has to (Palworld's `.sav`
/// header does). `src` must be exactly the compressed stream.
///
/// # Errors
///
/// Returns an [`Error`] if `src` is truncated, malformed, uses a part of the
/// format this crate does not cover, or does not decode to exactly
/// `dst.len()` bytes. `dst` may have been partly written.
///
/// # Example
///
/// ```
/// // A stored block: a two-byte header, then the bytes themselves.
/// let stream = [0xcc, 0x06, b'G', b'V', b'A', b'S'];
/// let mut out = [0u8; 4];
/// kraken_decode::decode(&stream, &mut out)?;
/// assert_eq!(&out, b"GVAS");
/// # Ok::<(), kraken_decode::Error>(())
/// ```
pub fn decode(src: &[u8], dst: &mut [u8]) -> Result<()> {
    let mut src_at = 0usize;
    let mut dst_at = 0usize;
    while dst_at < dst.len() {
        let block_len = BLOCK.min(dst.len() - dst_at);
        let header = src.get(src_at..src_at + 2).ok_or(Error::Truncated)?;
        let (flags, kind) = (header.first().copied().unwrap_or(0), header.get(1).copied().unwrap_or(0));
        src_at += 2;
        if flags & 0x0f != 0x0c {
            return Err(Error::Corrupt("block header"));
        }
        if flags & 0x30 != 0 {
            return Err(Error::Unsupported("block header flags"));
        }
        if kind & 0x80 != 0 {
            return Err(Error::Unsupported("block checksums"));
        }
        if !KNOWN_TYPES.contains(&kind) {
            return Err(Error::Unsupported("stream type"));
        }
        let block_end = dst_at + block_len;
        let out = dst.get_mut(dst_at..block_end).ok_or(Error::SizeMismatch)?;

        // A stored block: the bytes themselves.
        if flags & 0x40 != 0 {
            let raw = src.get(src_at..src_at + block_len).ok_or(Error::Truncated)?;
            out.copy_from_slice(raw);
            src_at += block_len;
            dst_at = block_end;
            continue;
        }

        // Beyond stored blocks, only one stream type is decoded.
        if kind != TYPE_BYTEWISE {
            return Err(Error::Unsupported("stream type"));
        }

        // Otherwise one group: a 3-byte big-endian word whose low 18 bits are
        // the group's size less one.
        let word = src.get(src_at..src_at + 3).ok_or(Error::Truncated)?;
        let word = word.iter().fold(0u32, |acc, &b| acc << 8 | u32::from(b));
        src_at += 3;
        let size_field = (word & 0x3_ffff) as usize;
        // The largest size with the bit above it set: the block is one byte, repeated.
        if size_field == 0x3_ffff {
            // A full block is never sent as a stored group (it would need this
            // size); it is sent as a stored block instead.
            if word & 0x4_0000 == 0 {
                return Err(Error::Corrupt("group header"));
            }
            let byte = *src.get(src_at).ok_or(Error::Truncated)?;
            out.fill(byte);
            src_at += 1;
            dst_at = block_end;
            continue;
        }
        let group_len = size_field + 1;
        let group = src.get(src_at..src_at + group_len).ok_or(Error::Truncated)?;
        src_at += group_len;
        match group_len.cmp(&block_len) {
            // Same size as the data: stored.
            Ordering::Equal => out.copy_from_slice(group),
            Ordering::Greater => return Err(Error::Corrupt("group larger than its block")),
            Ordering::Less => decode_group(group, dst, dst_at, block_len)?,
        }
        dst_at = block_end;
    }
    if src_at != src.len() {
        return Err(Error::SizeMismatch);
    }
    Ok(())
}

/// Decodes a group's chunks into `dst[start..start + len]`.
fn decode_group(group: &[u8], dst: &mut [u8], start: usize, len: usize) -> Result<()> {
    let mut at = 0usize;
    let mut done = 0usize;
    while done < len {
        let chunk_len = CHUNK.min(len - done);
        let chunk_start = start + done;
        let rest = group.get(at..).ok_or(Error::Truncated)?;
        let header = rest.get(..3).ok_or(Error::Truncated)?;
        let word = header.iter().fold(0u32, |acc, &b| acc << 8 | u32::from(b));
        if word & 0x80_0000 != 0 {
            // Matches and literals: a mode in 4 bits and a 19-bit size.
            let size = (word & 0x7_ffff) as usize;
            let mode = (word >> 19) & 0x0f;
            let body = rest.get(3..3 + size).ok_or(Error::Truncated)?;
            at += 3 + size;
            if size == chunk_len {
                // Same size as the data: stored.
                if mode != 0 {
                    return Err(Error::Corrupt("chunk header"));
                }
                dst.get_mut(chunk_start..chunk_start + chunk_len).ok_or(Error::SizeMismatch)?.copy_from_slice(body);
            } else {
                let delta_literals = match mode {
                    0 => true,
                    1 => false,
                    _ => return Err(Error::Unsupported("chunk mode")),
                };
                lz::decode_chunk(body, dst, chunk_start, chunk_len, delta_literals)?;
            }
        } else {
            // No matches: the chunk is a single coded array.
            if word & 0x70_0000 == 0 {
                return Err(Error::Unsupported("chunk header"));
            }
            let (bytes, used) = array::read(rest, chunk_len, 0)?;
            at += used;
            if bytes.len() != chunk_len {
                return Err(Error::Corrupt("chunk array length"));
            }
            dst.get_mut(chunk_start..chunk_start + chunk_len).ok_or(Error::SizeMismatch)?.copy_from_slice(&bytes);
        }
        done += chunk_len;
    }
    if at != group.len() {
        return Err(Error::Corrupt("group has bytes left over"));
    }
    Ok(())
}

/// Decodes `src` into a new vector of `decoded_len` bytes.
///
/// `decoded_len` comes from the container around the stream and sizes the
/// allocation, so check it against a limit of your own before calling this on
/// input you do not trust.
///
/// # Errors
///
/// As [`decode`].
pub fn decode_to_vec(src: &[u8], decoded_len: usize) -> Result<Vec<u8>> {
    let mut out = vec![0u8; decoded_len];
    decode(src, &mut out)?;
    Ok(out)
}
