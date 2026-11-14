//! The decoder reads untrusted bytes, so no input may make it panic, hang or
//! allocate without bound. These tests throw damaged streams at it: valid
//! ones from the test encoder with bytes flipped, cut, repeated and spliced,
//! and plain noise behind plausible headers.
//!
//! `KRAKEN_FUZZ_ITERS` sets how many inputs each test tries (default 3000).
//! The cargo-fuzz target in `fuzz/` does the same job with coverage guidance.

mod common;

use common::{encode, sample, ChunkKind, Coding, Options, Rng};
use kraken_decode::decode;
use proptest::prelude::*;

fn iterations() -> usize {
    std::env::var("KRAKEN_FUZZ_ITERS").ok().and_then(|v| v.parse().ok()).unwrap_or(3000)
}

fn seeds() -> Vec<(Vec<u8>, usize)> {
    let h3 = Coding::Huffman3 { rice_bits: 1 };
    let h6 = Coding::Huffman6 { rice_bits: 2 };
    let rle = |inner: Option<Coding>| Coding::RunLength { literals: inner.map(Box::new) };
    let chunks = [
        ChunkKind::Matches {
            literals: Coding::Stored,
            commands: Coding::Stored,
            near_split: None,
            delta_literals: false,
        },
        ChunkKind::Matches {
            literals: h6.clone(),
            commands: h3.clone(),
            near_split: Some((rle(None), h6.clone())),
            delta_literals: false,
        },
        ChunkKind::Matches {
            literals: rle(Some(h3.clone())),
            commands: rle(None),
            near_split: None,
            delta_literals: true,
        },
        ChunkKind::Array(h6.clone()),
        ChunkKind::Array(rle(Some(h6))),
        ChunkKind::Array(rle(None)),
    ];
    let mut out = Vec::new();
    for (index, chunk) in chunks.into_iter().enumerate() {
        for len in [600, 5_000, 70_000, 140_000] {
            let data = sample(len, (index * 31 + len) as u64);
            out.push((
                encode(&data, &Options { chunk: chunk.clone(), compact_headers: index % 2 == 0, min_match: 4 }),
                len,
            ));
        }
    }
    out
}

fn damage(rng: &mut Rng, stream: &mut Vec<u8>) {
    for _ in 0..=rng.below(4) {
        if stream.is_empty() {
            return;
        }
        // Damage near the front more often: that is where the headers are.
        let at = if rng.below(3) == 0 { rng.below(stream.len().min(64)) } else { rng.below(stream.len()) };
        match rng.below(7) {
            0 => stream[at] ^= 1 << rng.below(8),
            1 => stream[at] = rng.next() as u8,
            2 => stream[at] = [0, 1, 0x7f, 0x80, 0xff][rng.below(5)],
            3 => stream.truncate(at),
            4 => {
                let n = 1 + rng.below(8);
                let piece: Vec<u8> = (0..n).map(|_| rng.next() as u8).collect();
                stream.splice(at..at, piece);
            }
            5 => {
                let end = (at + 1 + rng.below(16)).min(stream.len());
                stream.drain(at..end);
            }
            _ => {
                let from = rng.below(stream.len());
                let n = (1 + rng.below(32)).min(stream.len() - from).min(stream.len() - at);
                stream.copy_within(from..from + n, at);
            }
        }
    }
}

#[test]
fn damaged_streams_never_panic() {
    let seeds = seeds();
    let mut rng = Rng(0x00c0_ffee);
    let mut out = vec![0u8; 150_000];
    let mut refused = 0usize;
    for round in 0..iterations() {
        let (seed, len) = &seeds[round % seeds.len()];
        let mut stream = seed.clone();
        damage(&mut rng, &mut stream);
        // Mostly the right length, sometimes a wrong one.
        let len = match rng.below(8) {
            0 => rng.below(out.len()),
            1 => (len + 1).min(out.len()),
            _ => *len,
        };
        if decode(&stream, &mut out[..len]).is_err() {
            refused += 1;
        }
    }
    // Sanity: the damage is real (most inputs are refused), not a no-op.
    assert!(refused * 2 > iterations(), "only {refused} of {} damaged streams were refused", iterations());
}

#[test]
fn noise_behind_valid_headers_never_panics() {
    let mut rng = Rng(0xdead_beef);
    let mut out = vec![0u8; 70_000];
    for _ in 0..iterations() {
        let len = 1 + rng.below(out.len() - 1);
        let body_len = rng.below(400);
        let mut stream = vec![0x8c, 10];
        // A group header that agrees with the body, then a chunk header of each kind in turn.
        let body: Vec<u8> = match rng.below(4) {
            0 => {
                let mut b = vec![0x88, (body_len >> 8) as u8, body_len as u8];
                b.extend((0..body_len).map(|_| rng.next() as u8));
                b
            }
            1 => {
                let word = (2 + 2 * rng.below(2) as u64) << 36 | ((len - 1) as u64) << 18 | body_len as u64;
                let mut b: Vec<u8> = (0..5).rev().map(|i| (word >> (8 * i)) as u8).collect();
                b.extend((0..body_len).map(|_| rng.next() as u8));
                b
            }
            2 => {
                let word = 3u64 << 36 | ((len - 1) as u64) << 18 | body_len as u64;
                let mut b: Vec<u8> = (0..5).rev().map(|i| (word >> (8 * i)) as u8).collect();
                b.extend((0..body_len).map(|_| if rng.below(3) == 0 { 0 } else { rng.next() as u8 }));
                b
            }
            _ => (0..body_len + 1).map(|_| rng.next() as u8).collect(),
        };
        let size = body.len() - 1;
        stream.extend([(size >> 16) as u8, (size >> 8) as u8, size as u8]);
        stream.extend(body);
        let _ = decode(&stream, &mut out[..len]);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn arbitrary_bytes_never_panic(stream in proptest::collection::vec(any::<u8>(), 0..600), len in 0usize..5000) {
        let mut out = vec![0u8; len];
        let _ = decode(&stream, &mut out);
    }

    #[test]
    fn any_data_round_trips(seed in any::<u64>(), len in 1usize..40_000, variant in 0usize..6, compact in any::<bool>()) {
        let data = sample(len, seed);
        let h6 = Coding::Huffman6 { rice_bits: (seed % 4) as u32 };
        let rle = |inner: Option<Coding>| Coding::RunLength { literals: inner.map(Box::new) };
        let chunk = match variant {
            0 => ChunkKind::Matches { literals: Coding::Stored, commands: Coding::Stored, near_split: None, delta_literals: seed & 16 != 0 },
            1 => ChunkKind::Matches { literals: h6.clone(), commands: h6, near_split: None, delta_literals: seed & 16 != 0 },
            2 => ChunkKind::Matches { literals: rle(None), commands: rle(Some(h6.clone())), near_split: Some((h6.clone(), h6)), delta_literals: false },
            3 => ChunkKind::Array(h6),
            4 => ChunkKind::Array(rle(Some(Coding::Huffman3 { rice_bits: 1 }))),
            _ => ChunkKind::Array(rle(None)),
        };
        let stream = encode(&data, &Options { chunk, compact_headers: compact, min_match: 3 + (seed % 3) as usize });
        let mut out = vec![0u8; len];
        prop_assert_eq!(decode(&stream, &mut out), Ok(()));
        prop_assert!(out == data);
    }
}
