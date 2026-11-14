//! The match-and-literal layer of a chunk.
//!
//! A chunk of up to 128 KiB is rebuilt from four parts: literal bytes, command
//! bytes, match offsets (16-bit "near" ones and wider "far" ones) and a few
//! lengths too long for a command byte. Commands run in two halves, one per
//! 64 KiB of output.

use alloc::vec::Vec;

use crate::array;
use crate::error::{Error, Result};

/// Output bytes covered by one half of a chunk.
const HALF: usize = 0x1_0000;

/// The smallest match offset.
const MIN_OFFSET: usize = 8;

/// A forward reader over a byte slice.
struct Bytes<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Bytes<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8]> {
        let end = self.at.checked_add(count).ok_or(Error::Truncated)?;
        let bytes = self.data.get(self.at..end).ok_or(Error::Truncated)?;
        self.at = end;
        Ok(bytes)
    }

    fn u8(&mut self) -> Result<u8> {
        self.take(1)?.first().copied().ok_or(Error::Truncated)
    }

    fn u16(&mut self) -> Result<u16> {
        let bytes = self.take(2)?;
        Ok(bytes.iter().rev().fold(0, |acc, &b| acc << 8 | u16::from(b)))
    }

    fn u24(&mut self) -> Result<u32> {
        let bytes = self.take(3)?;
        Ok(bytes.iter().rev().fold(0, |acc, &b| acc << 8 | u32::from(b)))
    }

    fn rest(&self) -> &'a [u8] {
        self.data.get(self.at..).unwrap_or_default()
    }

    /// An array, at most `max_len` bytes long.
    fn array(&mut self, max_len: usize) -> Result<alloc::borrow::Cow<'a, [u8]>> {
        let (bytes, used) = array::read(self.rest(), max_len, 0)?;
        self.at += used;
        Ok(bytes)
    }

    /// A length that did not fit its command: one byte, or for values above
    /// 251 that byte plus four times a following 16-bit number.
    fn long_length(&mut self) -> Result<usize> {
        let first = usize::from(self.u8()?);
        if first > 251 {
            Ok(first + 4 * usize::from(self.u16()?))
        } else {
            Ok(first)
        }
    }
}

/// A sequence consumed front to back.
struct Queue<'a, T> {
    items: &'a [T],
    at: usize,
}

impl<T: Copy> Queue<'_, T> {
    fn next(&mut self, what: &'static str) -> Result<T> {
        let item = self.items.get(self.at).copied().ok_or(Error::Corrupt(what))?;
        self.at += 1;
        Ok(item)
    }
}

/// The output buffer with the write position, and the copy rules.
struct Output<'a> {
    /// The whole decoded stream so far; matches may reach anywhere before `at`.
    buf: &'a mut [u8],
    at: usize,
    /// Distance of the most recent match.
    last_offset: usize,
    /// Literals are stored as differences from the byte `last_offset` back.
    delta_literals: bool,
}

impl Output<'_> {
    fn literals(&mut self, source: &mut Queue<'_, u8>, count: usize, limit: usize) -> Result<()> {
        let end = self.at.checked_add(count).filter(|&end| end <= limit);
        let end = end.ok_or(Error::Corrupt("literals cross a chunk boundary"))?;
        let from = source.items.get(source.at..).and_then(|s| s.get(..count));
        let from = from.ok_or(Error::Corrupt("literals exhausted"))?;
        source.at += count;
        if self.delta_literals {
            if self.last_offset > self.at {
                return Err(Error::Corrupt("match offset before the start"));
            }
            for &byte in from {
                let base = self.buf.get(self.at - self.last_offset).copied();
                let slot = self.buf.get_mut(self.at);
                let (Some(base), Some(slot)) = (base, slot) else { return Err(Error::SizeMismatch) };
                *slot = byte.wrapping_add(base);
                self.at += 1;
            }
        } else {
            self.buf.get_mut(self.at..end).ok_or(Error::SizeMismatch)?.copy_from_slice(from);
            self.at = end;
        }
        Ok(())
    }

    /// Copies `count` bytes from `offset` back. Source and destination may
    /// overlap, in which case the copy repeats what it has just written.
    fn copy(&mut self, offset: usize, count: usize, limit: usize) -> Result<()> {
        // The format has no offsets below 8: its matches are defined as copies
        // of 8 bytes at a time, which only equals a plain copy from 8 up.
        if offset < MIN_OFFSET {
            return Err(Error::Corrupt("match offset below 8"));
        }
        if offset > self.at {
            return Err(Error::Corrupt("match offset before the start"));
        }
        let end = self.at.checked_add(count).filter(|&end| end <= limit);
        let end = end.ok_or(Error::Corrupt("match crosses a chunk boundary"))?;
        if offset >= count {
            self.buf.copy_within(self.at - offset..self.at - offset + count, self.at);
        } else {
            for at in self.at..end {
                let byte = self.buf.get(at - offset).copied().ok_or(Error::SizeMismatch)?;
                *self.buf.get_mut(at).ok_or(Error::SizeMismatch)? = byte;
            }
        }
        self.at = end;
        self.last_offset = offset;
        Ok(())
    }
}

/// Decodes one chunk into `buf[start..start + len]`. `buf[..start]` must hold
/// the stream decoded so far. `delta_literals` is the chunk header's choice of
/// literal mode.
pub(crate) fn decode_chunk(src: &[u8], buf: &mut [u8], start: usize, len: usize, delta_literals: bool) -> Result<()> {
    let end = start.checked_add(len).filter(|&end| end <= buf.len()).ok_or(Error::SizeMismatch)?;
    let mut src = Bytes { data: src, at: 0 };
    let mut out = Output { buf, at: start, last_offset: 8, delta_literals };

    // The very first chunk of a stream opens with 8 bytes copied as they are,
    // which gives the initial match offset of 8 something to point at.
    if start == 0 {
        if len < 8 {
            return Err(Error::Corrupt("first chunk shorter than its raw prefix"));
        }
        let prefix = src.take(8)?;
        out.buf.get_mut(..8).ok_or(Error::SizeMismatch)?.copy_from_slice(prefix);
        out.at = 8;
    }

    let literals = src.array(len)?;
    let commands = src.array(len)?;
    // Where the second half's commands begin, when there is a second half.
    let second_half_at = if len > HALF {
        let at = usize::from(src.u16()?);
        if at > commands.len() {
            return Err(Error::Corrupt("command split past the end"));
        }
        at
    } else {
        commands.len()
    };

    // Near offsets: a count and that many 16-bit numbers, or, when the count
    // is 0xffff, two arrays holding the high and the low bytes.
    let near_count = usize::from(src.u16()?);
    let near: Vec<u16> = if near_count == 0xffff {
        let high = src.array(len)?;
        let low = src.array(len)?;
        if high.len() != low.len() {
            return Err(Error::Corrupt("near offset halves differ in length"));
        }
        high.iter().zip(low.iter()).map(|(&h, &l)| u16::from(h) << 8 | u16::from(l)).collect()
    } else {
        src.take(near_count * 2)?
            .chunks_exact(2)
            .map(|pair| pair.iter().rev().fold(0, |acc, &b| acc << 8 | u16::from(b)))
            .collect()
    };

    // Far offsets: one count per half packed in 24 bits (4095 means a 16-bit
    // count follows), then the offsets, three bytes each or four when large.
    let counts = src.u24()?;
    let mut far_counts = [(counts >> 12) as usize, (counts & 0xfff) as usize];
    for count in &mut far_counts {
        if *count == 0xfff {
            *count = usize::from(src.u16()?);
        }
    }
    let mut far = Vec::new();
    for _ in 0..far_counts[0] + far_counts[1] {
        let mut value = src.u24()?;
        if value >= 0xc0_0000 {
            value += u32::from(src.u8()?) << 22;
        }
        far.push(value);
    }
    let (far_first, far_second) = far.split_at_checked(far_counts[0]).ok_or(Error::Truncated)?;
    // Whatever is left holds the long lengths.
    let mut lengths = Bytes { data: src.rest(), at: 0 };

    let mut literals = Queue { items: &literals, at: 0 };
    let mut near = Queue { items: &near, at: 0 };
    let (first_commands, second_commands) = commands.split_at_checked(second_half_at).ok_or(Error::Truncated)?;
    let halves = [(first_commands, far_first), (second_commands, far_second)];
    for (index, (commands, far)) in halves.into_iter().enumerate() {
        let half_start = start + index * HALF;
        if half_start >= end {
            break;
        }
        let half_end = end.min(half_start + HALF);
        let mut far = Queue { items: far, at: 0 };
        for &command in commands {
            match command {
                // The common form: up to 7 literals, then a match of up to 15
                // bytes at a new near offset or, with the top bit, the last one.
                24..=255 => {
                    out.literals(&mut literals, usize::from(command & 7), half_end)?;
                    let offset = if command & 0x80 == 0 {
                        usize::from(near.next("near offsets exhausted")?)
                    } else {
                        out.last_offset
                    };
                    out.copy(offset, usize::from((command >> 3) & 15), half_end)?;
                }
                // A long run of literals.
                0 => {
                    let count = lengths.long_length()? + 64;
                    out.literals(&mut literals, count, half_end)?;
                }
                // A long match at a near offset.
                1 => {
                    let count = lengths.long_length()? + 91;
                    let offset = usize::from(near.next("near offsets exhausted")?);
                    out.copy(offset, count, half_end)?;
                }
                // A match at a far offset: long (2), or of 8 to 28 bytes. Far
                // offsets are stored less the distance already covered in this half.
                2..=23 => {
                    let count = if command == 2 { lengths.long_length()? + 29 } else { usize::from(command) + 5 };
                    let offset = far.next("far offsets exhausted")? as usize + (out.at - half_start);
                    out.copy(offset, count, half_end)?;
                }
            }
        }
        // Literals left after the last command fill the rest of the half.
        out.literals(&mut literals, half_end - out.at, half_end)?;
    }
    Ok(())
}
