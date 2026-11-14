//! Huffman-coded arrays.
//!
//! An array starts with a description of the code (which byte values occur and
//! how long each one's code is), padded to a whole byte, followed by the coded
//! bytes split over three or six interleaved bit streams.
//!
//! Codes are canonical, ordered by length and then by byte value, and at most
//! 11 bits long, so one 2048-entry table decodes any code in a single lookup.
//! See `docs/FORMAT.md` for the layout and `docs/SOURCES.md` for where each
//! rule comes from.

use crate::bits::{LsbCursor, MsbReader};
use crate::error::{Error, Result};

/// The longest code, in bits.
const MAX_CODE_BITS: u32 = 11;
const TABLE_SIZE: usize = 1 << MAX_CODE_BITS;

#[derive(Clone, Copy, Default)]
struct Entry {
    symbol: u8,
    /// Code length in bits, 1 to 11.
    bits: u8,
}

/// A decoding table: for every 11-bit window (first bit lowest), the byte
/// value whose code starts the window and that code's length.
pub(crate) struct Table {
    entries: [Entry; TABLE_SIZE],
}

impl Table {
    /// Reads a code description from the start of `data`. Returns the table
    /// and the number of whole bytes the description occupies.
    pub(crate) fn read(data: &[u8]) -> Result<(Self, usize)> {
        let mut reader = MsbReader::new(data);
        if reader.bits(2)? != 0b10 {
            return Err(Error::Unsupported("huffman table encoding"));
        }
        // Each code length is sent as a signed step from a running estimate,
        // as a Rice code: a unary part, then `rice_bits` low bits.
        let rice_bits = reader.bits(2)?;
        let symbol_count = reader.bits(8)? as usize + 1;
        let span_count = read_span_count(&mut reader, symbol_count)?;

        // The unary parts of every value come first: one per symbol, then one
        // per span of the list that says which byte values are in use.
        let mut length_unary = [0u8; 256];
        for slot in length_unary.iter_mut().take(symbol_count) {
            *slot = reader.unary(48)? as u8;
        }
        let mut span_unary = [0u8; 512];
        let span_unary = span_unary.get_mut(..span_count).ok_or(Error::Corrupt("huffman span count"))?;
        for slot in span_unary.iter_mut() {
            *slot = reader.unary(8)? as u8;
        }

        let mut lengths = [0u8; 256];
        let mut estimate: i32 = 30;
        for (slot, &unary) in lengths.iter_mut().zip(&length_unary).take(symbol_count) {
            let value = ((u32::from(unary) << rice_bits) | reader.bits(rice_bits)?) as i32;
            let step = if value & 1 == 1 { -((value + 1) >> 1) } else { value >> 1 };
            let length = step + (estimate >> 2) + 1;
            if !(1..=MAX_CODE_BITS as i32).contains(&length) {
                return Err(Error::Corrupt("huffman code length"));
            }
            *slot = length as u8;
            estimate += step;
        }

        // The byte values in use, as alternating spans: unused values to skip,
        // then used values to take. The list starts with a skip when its
        // length is odd, and the final "take" is whatever symbols remain.
        let mut symbols = [0u8; 256];
        let mut taken = 0usize;
        let mut next_value = 0usize;
        let starts_with_skip = span_count & 1 == 1;
        for (index, &unary) in span_unary.iter().enumerate() {
            let unary = u32::from(unary);
            let is_skip = (index & 1 == 0) == starts_with_skip;
            if is_skip {
                let skip = ((1u32 << (unary + 1)) | reader.bits(unary + 1)?) - 1;
                next_value += skip as usize;
            } else {
                let take = ((1u32 << unary) | reader.bits(unary)?) as usize;
                if taken + take > symbol_count || next_value + take > 256 {
                    return Err(Error::Corrupt("huffman symbol spans"));
                }
                for slot in symbols.iter_mut().skip(taken).take(take) {
                    *slot = next_value as u8;
                    next_value += 1;
                }
                taken += take;
            }
        }
        let rest = symbol_count - taken;
        if next_value + rest > 256 {
            return Err(Error::Corrupt("huffman symbol spans"));
        }
        for slot in symbols.iter_mut().skip(taken).take(rest) {
            *slot = next_value as u8;
            next_value += 1;
        }

        let symbols = symbols.get(..symbol_count).ok_or(Error::Corrupt("huffman symbol count"))?;
        let lengths = lengths.get(..symbol_count).ok_or(Error::Corrupt("huffman symbol count"))?;
        let table = Self::build(symbols, lengths)?;
        Ok((table, reader.bytes_consumed()))
    }

    /// Builds the lookup table for the canonical code with these lengths.
    /// `symbols` is ascending; `lengths[i]` belongs to `symbols[i]`.
    fn build(symbols: &[u8], lengths: &[u8]) -> Result<Self> {
        let mut count = [0u32; MAX_CODE_BITS as usize + 1];
        let mut space = 0u32;
        for &length in lengths {
            let length = u32::from(length);
            if let Some(slot) = count.get_mut(length as usize) {
                *slot += 1;
            }
            space += 1 << (MAX_CODE_BITS - length);
        }
        // A usable code fills the table exactly: no window left undefined and
        // none claimed twice.
        if space != TABLE_SIZE as u32 {
            return Err(Error::Corrupt("huffman code is not complete"));
        }
        let mut next_code = [0u32; MAX_CODE_BITS as usize + 2];
        let mut code = 0u32;
        for length in 1..=MAX_CODE_BITS as usize {
            if let (Some(slot), Some(&n)) = (next_code.get_mut(length), count.get(length)) {
                *slot = code;
                code = (code + n) << 1;
            }
        }
        let mut entries = [Entry::default(); TABLE_SIZE];
        for (&symbol, &bits) in symbols.iter().zip(lengths) {
            let length = u32::from(bits);
            let Some(slot) = next_code.get_mut(length as usize) else {
                return Err(Error::Corrupt("huffman code length"));
            };
            let code = *slot;
            *slot += 1;
            // The streams deliver a code's first bit lowest, so the window
            // holds the code mirrored, with any later bits above it.
            let mirrored = (code.reverse_bits() >> (32 - length)) as usize;
            let step = 1usize << length;
            let mut index = mirrored;
            while let Some(entry) = entries.get_mut(index) {
                *entry = Entry { symbol, bits };
                index += step;
            }
        }
        Ok(Self { entries })
    }

    /// Decodes `out.len()` bytes from one group of three streams.
    ///
    /// The group is a 16-bit size, the first stream of that size read forward,
    /// then a region shared by the third stream (read forward from its start)
    /// and the second (read backward from its end). Output bytes are dealt to
    /// the streams in turn: first, second, third, first, …
    fn decode_group(&self, data: &[u8], out: &mut [u8]) -> Result<()> {
        let (size, body) = data.split_first_chunk::<2>().ok_or(Error::Truncated)?;
        let first_len = usize::from(u16::from_le_bytes(*size));
        let (first, shared) = body.split_at_checked(first_len).ok_or(Error::Corrupt("huffman stream size"))?;
        let mut cursors = [LsbCursor::forward(first), LsbCursor::backward(shared), LsbCursor::forward(shared)];
        for (index, slot) in out.iter_mut().enumerate() {
            let Some(cursor) = cursors.get_mut(index % 3) else { continue };
            let entry = self.entries.get(cursor.peek11()).copied().unwrap_or_default();
            *slot = entry.symbol;
            cursor.skip(entry.bits);
        }
        let [a, b, c] = &cursors;
        if a.consumed() > first.len() * 8 || b.consumed() + c.consumed() > shared.len() * 8 {
            return Err(Error::Corrupt("huffman stream overrun"));
        }
        Ok(())
    }
}

/// How many spans describe the set of byte values in use. Sent as a number
/// below `2 * min(count, 257 - count)` in the fewest bits that can tell those
/// values apart (the low values get one bit fewer); absent when all 256 occur.
fn read_span_count(reader: &mut MsbReader<'_>, symbol_count: usize) -> Result<usize> {
    if symbol_count >= 256 {
        return Ok(0);
    }
    let choices = 2 * symbol_count.min(257 - symbol_count) as u32;
    let width = 32 - (choices - 1).leading_zeros();
    let short_codes = (1u32 << width) - choices;
    let mut value = reader.bits(width - 1)?;
    if value >= short_codes {
        value = ((value << 1) | reader.bit()?) - short_codes;
    }
    Ok(value as usize)
}

/// Decodes a Huffman array into `out`. `six_streams` selects the layout with
/// two independent groups of three streams, one per half of the output.
pub(crate) fn decode(data: &[u8], out: &mut [u8], six_streams: bool) -> Result<()> {
    let (table, used) = Table::read(data)?;
    let data = data.get(used..).ok_or(Error::Truncated)?;
    if !six_streams {
        return table.decode_group(data, out);
    }
    let (size, body) = data.split_first_chunk::<3>().ok_or(Error::Truncated)?;
    let first_len = usize::from(size[0]) | usize::from(size[1]) << 8 | usize::from(size[2]) << 16;
    let (first, second) = body.split_at_checked(first_len).ok_or(Error::Corrupt("huffman group size"))?;
    let (front, back) = out.split_at_mut(out.len().div_ceil(2));
    table.decode_group(first, front)?;
    table.decode_group(second, back)
}
