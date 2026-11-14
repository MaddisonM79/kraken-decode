//! Byte arrays.
//!
//! Everything a chunk carries besides a few raw fields is a byte array with a
//! short header saying how it is stored: as is, Huffman coded, or run-length
//! coded.

use alloc::borrow::Cow;
use alloc::vec;
use alloc::vec::Vec;

use crate::error::{Error, Result};
use crate::huffman;

/// How deep run-length arrays may nest (their literals are an array again).
const MAX_DEPTH: u8 = 4;

/// Reads the array at the start of `src`. Returns its bytes and how many bytes
/// of `src` it occupies. An array longer than `max_len` is refused.
pub(crate) fn read(src: &[u8], max_len: usize, depth: u8) -> Result<(Cow<'_, [u8]>, usize)> {
    let first = *src.first().ok_or(Error::Truncated)?;
    let kind = (first >> 4) & 7;
    let compact = first & 0x80 != 0;

    // Stored arrays: a 12-bit (compact) or 20-bit length, then the bytes.
    if kind == 0 {
        let (header, len) = if compact {
            let low = *src.get(1).ok_or(Error::Truncated)?;
            (2, usize::from(first & 0x0f) << 8 | usize::from(low))
        } else {
            let rest = src.get(1..3).ok_or(Error::Truncated)?;
            (3, rest.iter().fold(usize::from(first & 0x0f), |acc, &b| acc << 8 | usize::from(b)))
        };
        if len > max_len {
            return Err(Error::Corrupt("array longer than its chunk"));
        }
        let end = header + len;
        let bytes = src.get(header..end).ok_or(Error::Truncated)?;
        return Ok((Cow::Borrowed(bytes), end));
    }

    // Coded arrays: decoded and coded lengths, in 3 bytes (compact, 10 bits
    // each, the decoded one as its excess over the coded one) or 5 (18 bits each).
    let (header, decoded_len, coded_len) = if compact {
        let bytes = src.get(..3).ok_or(Error::Truncated)?;
        let word = bytes.iter().fold(0u32, |acc, &b| acc << 8 | u32::from(b));
        let coded = (word & 0x3ff) as usize;
        (3, coded + ((word >> 10) & 0x3ff) as usize + 1, coded)
    } else {
        let bytes = src.get(..5).ok_or(Error::Truncated)?;
        let word = bytes.iter().fold(0u64, |acc, &b| acc << 8 | u64::from(b));
        (5, ((word >> 18) & 0x3ffff) as usize + 1, (word & 0x3ffff) as usize)
    };
    if decoded_len > max_len {
        return Err(Error::Corrupt("array longer than its chunk"));
    }
    // An array that coding does not shrink is sent stored.
    if coded_len >= decoded_len {
        return Err(Error::Corrupt("coded array not smaller than its data"));
    }
    let end = header + coded_len;
    let payload = src.get(header..end).ok_or(Error::Truncated)?;
    let bytes = match kind {
        2 | 4 => {
            let mut out = vec![0u8; decoded_len];
            huffman::decode(payload, &mut out, kind == 4)?;
            out
        }
        3 => run_length(payload, decoded_len, depth)?,
        1 => return Err(Error::Unsupported("array type 1")),
        5 => return Err(Error::Unsupported("array type 5")),
        _ => return Err(Error::Corrupt("array type")),
    };
    Ok((Cow::Owned(bytes), end))
}

/// Decodes a run-length array of `decoded_len` bytes.
///
/// The array is literal bytes plus command bytes. Commands are read backward
/// from the end; each copies some literals, then repeats the current fill byte
/// (zero until a command changes it). When the first byte is zero, literals
/// follow it and meet the commands in the middle. Otherwise the literals are
/// an array of their own at the front, and the commands are everything after.
/// A payload of one byte is a special case: the whole array is that byte.
fn run_length(payload: &[u8], decoded_len: usize, depth: u8) -> Result<Vec<u8>> {
    let first = *payload.first().ok_or(Error::Truncated)?;
    // A payload of a single byte is that byte, repeated.
    if payload.len() == 1 {
        return Ok(vec![first; decoded_len]);
    }
    let inner;
    let (literals, commands, shared): (&[u8], &[u8], bool) = if first == 0 {
        let region = payload.get(1..).unwrap_or_default();
        (region, region, true)
    } else {
        if depth >= MAX_DEPTH {
            return Err(Error::Unsupported("array nesting"));
        }
        let (bytes, used) = read(payload, decoded_len, depth + 1)?;
        inner = bytes;
        (&inner, payload.get(used..).unwrap_or_default(), false)
    };

    let mut out = Vec::with_capacity(decoded_len);
    let mut fill = 0u8;
    let mut literal_at = 0usize;
    let mut command_at = commands.len();
    while out.len() < decoded_len {
        // In the shared layout a command byte may not come from the part the
        // literals have already used.
        let floor = if shared { literal_at } else { 0 };
        let mut next = || -> Result<u8> {
            if command_at <= floor {
                return Err(Error::Corrupt("run-length commands exhausted"));
            }
            command_at -= 1;
            commands.get(command_at).copied().ok_or(Error::Truncated)
        };
        let command = next()?;
        let (copy, repeat) = match command {
            // Short form: both counts in one byte.
            0x30..=0xff => (usize::from(!command & 0x0f), usize::from(command >> 4)),
            // Two bytes: 6 bits of literal count, 7 bits of repeat count.
            0x10..=0x2f => {
                let value = (usize::from(command) << 8 | usize::from(next()?)) - 0x1000;
                (value & 0x3f, value >> 6)
            }
            // Long repeat, in units of 128.
            0x09..=0x0f => (0, ((usize::from(command - 0x09) << 8 | usize::from(next()?)) + 1) * 128),
            // Long literal copy, in units of 64.
            0x02..=0x08 => (((usize::from(command - 0x02) << 8 | usize::from(next()?)) + 1) * 64, 0),
            // The next literal becomes the fill byte.
            0x01 => {
                let ceiling = if shared { command_at } else { literals.len() };
                if literal_at >= ceiling {
                    return Err(Error::Corrupt("run-length literals exhausted"));
                }
                fill = literals.get(literal_at).copied().ok_or(Error::Truncated)?;
                literal_at += 1;
                continue;
            }
            0x00 => (15, 0),
        };
        let ceiling = if shared { command_at } else { literals.len() };
        let literal_end = literal_at.checked_add(copy).filter(|&end| end <= ceiling);
        let Some(literal_end) = literal_end else {
            return Err(Error::Corrupt("run-length literals exhausted"));
        };
        if copy + repeat > decoded_len - out.len() {
            return Err(Error::Corrupt("run-length output overrun"));
        }
        out.extend_from_slice(literals.get(literal_at..literal_end).ok_or(Error::Truncated)?);
        literal_at = literal_end;
        out.resize(out.len() + repeat, fill);
    }
    let literals_end = if shared { command_at } else { literals.len() };
    if literal_at != literals_end || (!shared && command_at != 0) {
        return Err(Error::Corrupt("run-length array has bytes left over"));
    }
    Ok(out)
}
