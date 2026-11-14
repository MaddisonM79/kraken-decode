//! A small encoder for the stream format, for tests only.
//!
//! The crate decodes and nothing else, and no real save may be committed, so
//! the tests need a way to make valid streams from data that is safe to
//! publish. This encoder is that: it aims to reach every path of the decoder,
//! not to compress well. Its output was checked against the reference decoder
//! (as a black box, see `tools/parity.mjs`), which is what makes a round trip
//! through it meaningful.

#![allow(dead_code, clippy::manual_is_multiple_of)]

/// How to store one byte array.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Coding {
    /// As is.
    Stored,
    /// Huffman, three streams.
    Huffman3 { rice_bits: u32 },
    /// Huffman, six streams.
    Huffman6 { rice_bits: u32 },
    /// Run-length, with the literals in place or as an array of their own.
    RunLength { literals: Option<Box<Coding>> },
}

/// How to store a chunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChunkKind {
    /// Matches and literals.
    Matches { literals: Coding, commands: Coding, near_split: Option<(Coding, Coding)>, delta_literals: bool },
    /// One coded array, no matches.
    Array(Coding),
    /// As is.
    Stored,
}

/// How to store a stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    pub chunk: ChunkKind,
    /// Use short array headers where they fit.
    pub compact_headers: bool,
    /// Shortest match the parser takes.
    pub min_match: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            chunk: ChunkKind::Matches {
                literals: Coding::Stored,
                commands: Coding::Stored,
                near_split: None,
                delta_literals: false,
            },
            compact_headers: false,
            min_match: 4,
        }
    }
}

const BLOCK: usize = 0x4_0000;
const CHUNK: usize = 0x2_0000;
const HALF: usize = 0x1_0000;

/// Encodes `data` as a stream of type 10.
pub fn encode(data: &[u8], options: &Options) -> Vec<u8> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < data.len() {
        let len = BLOCK.min(data.len() - at);
        let block = &data[at..at + len];
        let header = out.len();
        out.extend([if at == 0 { 0x8c } else { 0x0c }, 10]);
        if len > 1 && block.iter().all(|&b| b == block[0]) {
            // One byte, repeated.
            out.extend([0x07, 0xff, 0xff, block[0]]);
        } else {
            let mut group = Vec::new();
            let mut done = 0;
            while done < len {
                let chunk_len = CHUNK.min(len - done);
                group.extend(encode_chunk(data, at + done, chunk_len, options));
                done += chunk_len;
            }
            if group.len() >= len && len == BLOCK {
                // No gain on a full block: a stored block.
                out[header] |= 0x40;
                out.extend(block);
                at += len;
                continue;
            }
            if group.len() >= len {
                // No gain: a group the size of its data is read as stored.
                group = block.to_vec();
            }
            let size = group.len() - 1;
            out.extend([(size >> 16) as u8, (size >> 8) as u8, size as u8]);
            out.extend(group);
        }
        at += len;
    }
    out
}

/// A stream of stored blocks, as `0xcc` then the type byte then the data.
pub fn encode_stored(data: &[u8], kind: u8) -> Vec<u8> {
    let mut out = Vec::new();
    for block in data.chunks(BLOCK) {
        out.extend([0xcc, kind]);
        out.extend(block);
    }
    out
}

fn stored_chunk(data: &[u8]) -> Vec<u8> {
    let word = 0x80_0000 | data.len();
    let mut out = vec![(word >> 16) as u8, (word >> 8) as u8, word as u8];
    out.extend(data);
    out
}

fn encode_chunk(all: &[u8], start: usize, len: usize, options: &Options) -> Vec<u8> {
    let data = &all[start..start + len];
    let body = match &options.chunk {
        ChunkKind::Stored => None,
        ChunkKind::Array(coding) => {
            let array = encode_array(data, coding, false, true);
            // A chunk-level array must be coded, with the long header.
            return if array[0] >> 4 == 0 || array.len() >= len { stored_chunk(data) } else { array };
        }
        ChunkKind::Matches { literals, commands, near_split, delta_literals } => {
            encode_matches(all, start, len, literals, commands, near_split.as_ref(), *delta_literals, options)
        }
    };
    match body {
        Some((body, delta)) if body.len() < len => {
            let word = 0x80_0000 | usize::from(!delta) << 19 | body.len();
            let mut out = vec![(word >> 16) as u8, (word >> 8) as u8, word as u8];
            out.extend(body);
            out
        }
        _ => stored_chunk(data),
    }
}

// ---------------------------------------------------------------- matches

struct Parts {
    literals: Vec<u8>,
    commands: Vec<u8>,
    near: Vec<u16>,
    far: [Vec<u32>; 2],
    lengths: Vec<u8>,
    second_half_at: usize,
}

fn long_length(lengths: &mut Vec<u8>, value: usize) {
    if value <= 251 {
        lengths.push(value as u8);
    } else {
        let extra = (value - 252) / 4;
        lengths.push((value - 4 * extra) as u8);
        lengths.extend((extra as u16).to_le_bytes());
    }
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn encode_matches(
    all: &[u8],
    start: usize,
    len: usize,
    literals: &Coding,
    commands: &Coding,
    near_split: Option<&(Coding, Coding)>,
    delta_literals: bool,
    options: &Options,
) -> Option<(Vec<u8>, bool)> {
    let end = start + len;
    let mut body = Vec::new();
    let mut at = start;
    if start == 0 {
        if len < 8 {
            return None;
        }
        body.extend(&all[..8]);
        at = 8;
    }
    let mut parts = Parts {
        literals: vec![],
        commands: vec![],
        near: vec![],
        far: [vec![], vec![]],
        lengths: vec![],
        second_half_at: 0,
    };
    let mut last_offset = 8usize;
    // Most recent position of each 4-byte sequence, over the whole stream so far.
    let mut seen: std::collections::HashMap<[u8; 4], usize> = std::collections::HashMap::new();
    let key = |p: usize| -> Option<[u8; 4]> { all.get(p..p + 4).map(|s| [s[0], s[1], s[2], s[3]]) };
    for p in 0..at.saturating_sub(3) {
        if let Some(k) = key(p) {
            seen.insert(k, p);
        }
    }
    let mut indexed = at.saturating_sub(3);

    for half in 0..2 {
        let half_start = start + half * HALF;
        if half_start >= end {
            break;
        }
        let half_end = end.min(half_start + HALF);
        if half == 1 {
            parts.second_half_at = parts.commands.len();
        }
        let mut literal_from = at;
        let literal = |parts: &mut Parts, p: usize, last: usize| {
            parts.literals.push(if delta_literals { all[p].wrapping_sub(all[p - last]) } else { all[p] });
        };
        while at < half_end {
            while indexed < at {
                if let Some(k) = key(indexed) {
                    seen.insert(k, indexed);
                }
                indexed += 1;
            }
            // Longest of: the last offset again, or the most recent same 4 bytes.
            let run = |offset: usize| -> usize {
                // The format has no offsets below 8.
                if offset < 8 || offset > at {
                    return 0;
                }
                (at..half_end).take_while(|&p| all[p] == all[p - offset]).count()
            };
            let mut best = (run(last_offset), last_offset);
            if let Some(&candidate) = key(at).and_then(|k| seen.get(&k)) {
                let found = run(at - candidate);
                if found > best.0 {
                    best = (found, at - candidate);
                }
            }
            let (mut count, offset) = best;
            let is_far = offset > 0xffff && offset != last_offset;
            let enough = if is_far {
                8
            } else if offset == last_offset {
                2
            } else {
                options.min_match.max(3)
            };
            if count < enough {
                at += 1;
                continue;
            }
            // Flush literals so that at most 7 ride on the match command.
            let mut pending = at - literal_from;
            while pending > 7 || (is_far && pending > 0) || (pending > 0 && count >= 91 && offset != last_offset) {
                if pending >= 64 {
                    parts.commands.push(0);
                    long_length(&mut parts.lengths, pending - 64);
                    for p in literal_from..at {
                        literal(&mut parts, p, last_offset);
                    }
                    literal_from = at;
                    pending = 0;
                } else {
                    let take = pending.min(7);
                    parts.commands.push(0x80 | take as u8);
                    for p in literal_from..literal_from + take {
                        literal(&mut parts, p, last_offset);
                    }
                    literal_from += take;
                    pending -= take;
                }
            }
            for p in literal_from..at {
                literal(&mut parts, p, last_offset);
            }
            if is_far {
                let stored = (offset - (at - half_start)) as u32;
                parts.far[half].push(stored);
                if count >= 29 {
                    parts.commands.push(2);
                    long_length(&mut parts.lengths, count - 29);
                } else {
                    parts.commands.push((count - 5) as u8);
                }
                at += count;
                count = 0;
            } else if offset != last_offset && count >= 91 {
                parts.commands.push(1);
                long_length(&mut parts.lengths, count - 91);
                parts.near.push(offset as u16);
                at += count;
                count = 0;
            } else {
                let first = count.min(15);
                let repeat = if offset == last_offset { 0x80 } else { 0 };
                parts.commands.push(repeat | (first as u8) << 3 | pending as u8);
                if repeat == 0 {
                    parts.near.push(offset as u16);
                }
                at += first;
                count -= first;
            }
            last_offset = offset;
            // Anything left continues at the same offset.
            while count > 0 {
                let piece = count.min(15);
                parts.commands.push(0x80 | (piece as u8) << 3);
                at += piece;
                count -= piece;
            }
            literal_from = at;
        }
        // The half's trailing literals need no command.
        for p in literal_from..half_end {
            literal(&mut parts, p, last_offset);
        }
        at = half_end;
    }
    if len <= HALF {
        parts.second_half_at = parts.commands.len();
    }

    let compact = options.compact_headers;
    body.extend(encode_array(&parts.literals, literals, compact, false));
    body.extend(encode_array(&parts.commands, commands, compact, false));
    if len > HALF {
        body.extend((parts.second_half_at as u16).to_le_bytes());
    }
    match near_split {
        Some((high, low)) if !parts.near.is_empty() => {
            body.extend([0xff, 0xff]);
            body.extend(encode_array(
                &parts.near.iter().map(|&o| (o >> 8) as u8).collect::<Vec<_>>(),
                high,
                compact,
                false,
            ));
            body.extend(encode_array(&parts.near.iter().map(|&o| o as u8).collect::<Vec<_>>(), low, compact, false));
        }
        _ => {
            if parts.near.len() >= 0xffff {
                return None;
            }
            body.extend((parts.near.len() as u16).to_le_bytes());
            for offset in &parts.near {
                body.extend(offset.to_le_bytes());
            }
        }
    }
    let counts = [parts.far[0].len(), parts.far[1].len()];
    let packed = counts[0].min(0xfff) << 12 | counts[1].min(0xfff);
    body.extend([packed as u8, (packed >> 8) as u8, (packed >> 16) as u8]);
    for count in counts {
        if count >= 0xfff {
            body.extend((count as u16).to_le_bytes());
        }
    }
    for &value in parts.far.iter().flatten() {
        if value < 0xc0_0000 {
            body.extend([value as u8, (value >> 8) as u8, (value >> 16) as u8]);
        } else {
            let high = (value - 0xc0_0000) >> 22;
            let low = value - (high << 22);
            body.extend([low as u8, (low >> 8) as u8, (low >> 16) as u8, high as u8]);
        }
    }
    body.extend(parts.lengths);
    Some((body, delta_literals))
}

// ----------------------------------------------------------------- arrays

/// Encodes a byte array. Falls back to stored when the coding cannot hold it.
pub fn encode_array(data: &[u8], coding: &Coding, compact: bool, top_level: bool) -> Vec<u8> {
    let payload = match coding {
        Coding::Stored => None,
        Coding::Huffman3 { rice_bits } => huffman(data, *rice_bits, false).map(|p| (2u64, p)),
        Coding::Huffman6 { rice_bits } => huffman(data, *rice_bits, true).map(|p| (4u64, p)),
        Coding::RunLength { literals } => run_length(data, literals.as_deref(), compact).map(|p| (3u64, p)),
    };
    match payload {
        // A coded array must come out smaller than its data, or it is stored instead.
        Some((kind, payload)) if data.len() <= 0x4_0000 && payload.len() < data.len() => {
            let excess = data.len().checked_sub(payload.len() + 1);
            let mut out = Vec::new();
            match excess {
                Some(excess) if compact && !top_level && payload.len() < 0x400 && excess < 0x400 => {
                    let word = 0x80_0000 | kind << 20 | (excess as u64) << 10 | payload.len() as u64;
                    out.extend([(word >> 16) as u8, (word >> 8) as u8, word as u8]);
                }
                _ => {
                    let word = kind << 36 | ((data.len() - 1) as u64) << 18 | payload.len() as u64;
                    out.extend((0..5).rev().map(|i| (word >> (8 * i)) as u8));
                }
            }
            out.extend(payload);
            out
        }
        _ => {
            let mut out = if compact && !top_level && data.len() < 0x1000 {
                vec![0x80 | (data.len() >> 8) as u8, data.len() as u8]
            } else {
                vec![(data.len() >> 16) as u8, (data.len() >> 8) as u8, data.len() as u8]
            };
            out.extend(data);
            out
        }
    }
}

struct MsbWriter {
    bytes: Vec<u8>,
    bits: usize,
}

impl MsbWriter {
    fn put(&mut self, value: u32, count: u32) {
        for i in (0..count).rev() {
            if self.bits % 8 == 0 {
                self.bytes.push(0);
            }
            let last = self.bytes.len() - 1;
            self.bytes[last] |= (((value >> i) & 1) as u8) << (7 - self.bits % 8);
            self.bits += 1;
        }
    }

    fn unary(&mut self, zeros: u32) {
        for _ in 0..zeros {
            self.put(0, 1);
        }
        self.put(1, 1);
    }
}

/// Code lengths of at most 11 bits for the byte values in `data`.
fn code_lengths(data: &[u8]) -> Option<Vec<(u8, u8)>> {
    let mut weight = [0u64; 256];
    for &b in data {
        weight[usize::from(b)] += 1;
    }
    let symbols: Vec<u8> = (0..=255u8).filter(|&s| weight[usize::from(s)] > 0).collect();
    if symbols.len() < 2 {
        return None;
    }
    loop {
        // Plain Huffman: merge the two lightest until one tree is left.
        let mut nodes: Vec<(u64, Vec<usize>)> =
            symbols.iter().enumerate().map(|(i, &s)| (weight[usize::from(s)], vec![i])).collect();
        let mut depth = vec![0u8; symbols.len()];
        while nodes.len() > 1 {
            nodes.sort_by_key(|node| std::cmp::Reverse(node.0));
            let (a, b) = (nodes.pop()?, nodes.pop()?);
            for &i in a.1.iter().chain(&b.1) {
                depth[i] += 1;
            }
            nodes.push((a.0 + b.0, [a.1, b.1].concat()));
        }
        if depth.iter().all(|&d| d <= 11) {
            return Some(symbols.iter().copied().zip(depth).collect());
        }
        // Too deep: flatten the weights and try again.
        for &s in &symbols {
            weight[usize::from(s)] = weight[usize::from(s)].div_ceil(2);
        }
    }
}

fn huffman(data: &[u8], rice_bits: u32, six_streams: bool) -> Option<Vec<u8>> {
    let lengths = code_lengths(data)?;
    let count = lengths.len();
    let mut table = MsbWriter { bytes: vec![], bits: 0 };
    table.put(0b10, 2);
    table.put(rice_bits, 2);
    table.put(count as u32 - 1, 8);

    // Spans of unused and used byte values.
    let mut spans: Vec<(bool, u32)> = Vec::new();
    let mut next = 0u32;
    let mut i = 0;
    while i < count {
        let symbol = u32::from(lengths[i].0);
        if symbol > next {
            spans.push((true, symbol - next));
        }
        let mut j = i;
        while j + 1 < count && lengths[j + 1].0 == lengths[j].0 + 1 {
            j += 1;
        }
        spans.push((false, (j - i + 1) as u32));
        next = u32::from(lengths[j].0) + 1;
        i = j + 1;
    }
    spans.pop(); // the final run of used values is implied
    if count < 256 {
        let choices = 2 * count.min(257 - count) as u32;
        let width = 32 - (choices - 1).leading_zeros();
        let short_codes = (1 << width) - choices;
        let value = spans.len() as u32;
        if value < short_codes {
            table.put(value, width - 1);
        } else {
            table.put(value + short_codes, width);
        }
    }

    let mut estimate = 30i32;
    let mut values = Vec::new();
    for &(_, length) in &lengths {
        let step = i32::from(length) - ((estimate >> 2) + 1);
        values.push(if step >= 0 { 2 * step } else { -2 * step - 1 } as u32);
        estimate += step;
    }
    for &value in &values {
        table.unary(value >> rice_bits);
    }
    let span_unary =
        |&(skip, n): &(bool, u32)| if skip { 30 - (n + 1).leading_zeros() } else { 31 - n.leading_zeros() };
    for span in &spans {
        table.unary(span_unary(span));
    }
    for &value in &values {
        table.put(value & ((1 << rice_bits) - 1), rice_bits);
    }
    for span in &spans {
        let unary = span_unary(span);
        if span.0 {
            table.put(span.1 + 1, unary + 1);
        } else {
            table.put(span.1, unary);
        }
    }

    // Canonical codes: by length, then by byte value.
    let mut order: Vec<(u8, u8)> = lengths.iter().map(|&(symbol, length)| (length, symbol)).collect();
    order.sort_unstable();
    let mut codes = [(0u32, 0u8); 256];
    let mut code = 0u32;
    let mut previous = order[0].0;
    for (length, symbol) in order {
        code <<= length - previous;
        previous = length;
        codes[usize::from(symbol)] = (code, length);
        code += 1;
    }

    let group = |data: &[u8]| -> Option<Vec<u8>> {
        let mut streams = [(Vec::<u8>::new(), 0usize), (Vec::new(), 0), (Vec::new(), 0)];
        for (index, &byte) in data.iter().enumerate() {
            let (bytes, bits) = &mut streams[index % 3];
            let (code, length) = codes[usize::from(byte)];
            for i in (0..length).rev() {
                if *bits % 8 == 0 {
                    bytes.push(0);
                }
                let last = bytes.len() - 1;
                bytes[last] |= (((code >> i) & 1) as u8) << (*bits % 8);
                *bits += 1;
            }
        }
        let [(first, _), (mut second, _), (third, _)] = streams;
        let mut out = u16::try_from(first.len()).ok()?.to_le_bytes().to_vec();
        out.extend(first);
        out.extend(third);
        second.reverse();
        out.extend(second);
        Some(out)
    };
    let mut out = table.bytes;
    if six_streams {
        let (front, back) = data.split_at(data.len().div_ceil(2));
        let first = group(front)?;
        out.extend([first.len() as u8, (first.len() >> 8) as u8, (first.len() >> 16) as u8]);
        out.extend(first);
        out.extend(group(back)?);
    } else {
        out.extend(group(data)?);
    }
    Some(out)
}

fn run_length(data: &[u8], inner: Option<&Coding>, compact: bool) -> Option<Vec<u8>> {
    // One byte value throughout: the payload is just that byte.
    if data.len() >= 16 && data.iter().all(|&b| b == data[0]) {
        return Some(vec![data[0]]);
    }
    let mut literals = Vec::new();
    // Command bytes in the order the decoder reads them.
    let mut commands = Vec::new();
    let mut fill = 0u8;
    let mut at = 0;
    let mut pending = 0usize; // literals waiting for a command
    let emit = |commands: &mut Vec<u8>, mut copy: usize, mut repeat: usize| {
        while copy >= 64 {
            let units = (copy / 64).min(7 * 256);
            commands.extend([0x02 + ((units - 1) >> 8) as u8, (units - 1) as u8]);
            copy -= units * 64;
        }
        loop {
            let now = repeat.min(127);
            if copy == 15 && now == 0 {
                commands.push(0x00);
            } else if copy <= 15 && (3..=15).contains(&now) {
                commands.push((now as u8) << 4 | (15 - copy as u8));
            } else if copy > 0 || now > 0 {
                let value = 0x1000 + (copy | now << 6);
                commands.extend([(value >> 8) as u8, value as u8]);
            }
            copy = 0;
            repeat -= now;
            if repeat < 128 {
                if repeat == 0 {
                    break;
                }
                continue;
            }
            let units = (repeat / 128).min(7 * 256);
            commands.extend([0x09 + ((units - 1) >> 8) as u8, (units - 1) as u8]);
            repeat -= units * 128;
            if repeat == 0 {
                break;
            }
        }
    };
    while at < data.len() {
        let run = data[at..].iter().take_while(|&&b| b == data[at]).count();
        if run >= 4 {
            if data[at] != fill {
                // Literals copied so far must be flushed before the fill byte changes.
                if pending > 0 {
                    emit(&mut commands, pending, 0);
                    pending = 0;
                }
                commands.push(0x01);
                fill = data[at];
                literals.push(fill);
            }
            emit(&mut commands, pending, run);
            pending = 0;
            at += run;
        } else {
            literals.extend(&data[at..at + run]);
            pending += run;
            at += run;
        }
    }
    if pending > 0 {
        emit(&mut commands, pending, 0);
    }
    commands.reverse();
    let mut out = match inner {
        None => {
            let mut out = vec![0];
            out.extend(literals);
            out
        }
        Some(coding) => {
            let array = encode_array(&literals, coding, compact, false);
            if array[0] == 0 {
                return None; // would be read as the in-place layout
            }
            array
        }
    };
    out.extend(commands);
    Some(out)
}

// ------------------------------------------------------------------- data

/// A small deterministic generator, so test data needs no dependency and no file.
pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// Bytes shaped like serialised game data: repeated names, small integers,
/// zero runs, some noise, and the occasional copy from far back.
pub fn sample(len: usize, seed: u64) -> Vec<u8> {
    const WORDS: [&[u8]; 8] = [
        b"StructProperty\0",
        b"IntProperty\0",
        b"Name\0",
        b"ArrayProperty\0",
        b"Guid\0",
        b"None\0",
        b"SaveData\0",
        b"Value\0",
    ];
    let mut rng = Rng(seed | 1);
    let mut out = b"GVAS".to_vec();
    while out.len() < len {
        match rng.below(10) {
            0..=3 => out.extend(WORDS[rng.below(WORDS.len())]),
            4 => out.extend(std::iter::repeat_n(0, 1 + rng.below(300))),
            5 => out.extend((rng.below(2000) as u32).to_le_bytes()),
            6 => out.extend((0..rng.below(24)).map(|_| rng.next() as u8)),
            7 => out.extend(std::iter::repeat_n(rng.next() as u8, 4 + rng.below(40))),
            _ => {
                // Repeat an earlier stretch, sometimes from far back and long.
                let n = if rng.below(8) == 0 { 100 + rng.below(4000) } else { 4 + rng.below(60) };
                let from = rng.below(out.len());
                for i in 0..n {
                    let b = out[(from + i).min(out.len() - 1)];
                    out.push(b);
                }
            }
        }
    }
    out.truncate(len);
    out
}
