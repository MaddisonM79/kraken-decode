//! Bit readers.
//!
//! Two bit orders are in use. Table descriptions are read most significant
//! bit first ([`MsbReader`]). Huffman code streams are read least significant
//! bit first, either walking forward from the start of a byte range or
//! backward from its end ([`LsbCursor`]).

use crate::error::{Error, Result};

/// Reads bits most significant first from a byte slice.
pub(crate) struct MsbReader<'a> {
    data: &'a [u8],
    /// Position in bits from the start of `data`.
    pos: usize,
}

impl<'a> MsbReader<'a> {
    pub(crate) fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    /// One bit, or `Truncated` past the end.
    pub(crate) fn bit(&mut self) -> Result<u32> {
        let byte = *self.data.get(self.pos >> 3).ok_or(Error::Truncated)?;
        let bit = (byte >> (7 - (self.pos & 7))) & 1;
        self.pos += 1;
        Ok(u32::from(bit))
    }

    /// `count` bits (at most 24) as an unsigned number, first bit highest.
    pub(crate) fn bits(&mut self, count: u32) -> Result<u32> {
        debug_assert!(count <= 24);
        let mut value = 0u32;
        for _ in 0..count {
            value = (value << 1) | self.bit()?;
        }
        Ok(value)
    }

    /// The number of zero bits before the next one bit, which is consumed.
    /// More than `limit` zeros is corrupt.
    pub(crate) fn unary(&mut self, limit: u32) -> Result<u32> {
        let mut zeros = 0u32;
        while self.bit()? == 0 {
            zeros += 1;
            if zeros > limit {
                return Err(Error::Corrupt("unary code too long"));
            }
        }
        Ok(zeros)
    }

    /// Bytes consumed so far, counting a partly read byte as whole.
    pub(crate) fn bytes_consumed(&self) -> usize {
        self.pos.div_ceil(8)
    }
}

/// A least-significant-bit-first cursor over a byte range, forward or backward.
///
/// Forward, bit `i` is bit `i % 8` of byte `i / 8`. Backward, bit `i` is bit
/// `i % 8` of the byte `i / 8` places before the end. Peeking past the range
/// yields zero bits; the caller checks [`LsbCursor::consumed`] against the
/// range's size afterwards.
pub(crate) struct LsbCursor<'a> {
    data: &'a [u8],
    backward: bool,
    pos: usize,
}

impl<'a> LsbCursor<'a> {
    pub(crate) fn forward(data: &'a [u8]) -> Self {
        Self { data, backward: false, pos: 0 }
    }

    pub(crate) fn backward(data: &'a [u8]) -> Self {
        Self { data, backward: true, pos: 0 }
    }

    fn byte(&self, index: usize) -> u32 {
        let at = if self.backward {
            match self.data.len().checked_sub(index + 1) {
                Some(at) => at,
                None => return 0,
            }
        } else {
            index
        };
        u32::from(self.data.get(at).copied().unwrap_or(0))
    }

    /// The next 11 bits without consuming them, first bit lowest.
    pub(crate) fn peek11(&self) -> usize {
        let first = self.pos >> 3;
        let window = self.byte(first) | (self.byte(first + 1) << 8) | (self.byte(first + 2) << 16);
        ((window >> (self.pos & 7)) & 0x7ff) as usize
    }

    pub(crate) fn skip(&mut self, bits: u8) {
        self.pos += usize::from(bits);
    }

    /// Bits consumed so far.
    pub(crate) fn consumed(&self) -> usize {
        self.pos
    }
}
