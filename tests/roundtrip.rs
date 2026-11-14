//! Round trips through the test encoder (`tests/common`).

mod common;

use common::{encode, encode_stored, sample, ChunkKind, Coding, Options};
use kraken_decode::{decode_to_vec, Error};

fn matches(literals: Coding, commands: Coding) -> ChunkKind {
    ChunkKind::Matches { literals, commands, near_split: None, delta_literals: false }
}

fn huff3() -> Coding {
    Coding::Huffman3 { rice_bits: 1 }
}

fn huff6() -> Coding {
    Coding::Huffman6 { rice_bits: 2 }
}

fn rle(inner: Option<Coding>) -> Coding {
    Coding::RunLength { literals: inner.map(Box::new) }
}

/// Every way of storing a chunk that the encoder knows.
pub fn variants() -> Vec<Options> {
    let mut chunks = vec![
        ChunkKind::Stored,
        matches(Coding::Stored, Coding::Stored),
        matches(huff3(), huff3()),
        matches(huff6(), huff6()),
        matches(Coding::Stored, rle(None)),
        matches(rle(None), rle(Some(huff6()))),
        matches(rle(Some(huff3())), Coding::Huffman6 { rice_bits: 0 }),
        matches(rle(Some(rle(None))), Coding::Huffman3 { rice_bits: 3 }),
        ChunkKind::Matches {
            literals: huff6(),
            commands: huff6(),
            near_split: Some((rle(None), huff6())),
            delta_literals: false,
        },
        ChunkKind::Matches {
            literals: Coding::Stored,
            commands: Coding::Stored,
            near_split: Some((Coding::Stored, huff3())),
            delta_literals: true,
        },
        ChunkKind::Matches { literals: huff6(), commands: rle(Some(huff6())), near_split: None, delta_literals: true },
        ChunkKind::Array(huff6()),
        ChunkKind::Array(huff3()),
        ChunkKind::Array(rle(None)),
        ChunkKind::Array(rle(Some(huff6()))),
    ];
    let mut out = Vec::new();
    for chunk in chunks.drain(..) {
        for compact_headers in [false, true] {
            out.push(Options { chunk: chunk.clone(), compact_headers, min_match: 4 });
        }
    }
    out
}

#[test]
fn every_variant_round_trips() {
    // Sizes around the chunk (128 KiB), half (64 KiB) and block (256 KiB) edges.
    for len in [1, 7, 8, 9, 300, 4096, 65_535, 65_536, 65_537, 131_072, 131_073, 262_144, 262_145, 700_000] {
        let data = sample(len, len as u64);
        for options in variants() {
            let stream = encode(&data, &options);
            let back = decode_to_vec(&stream, data.len()).unwrap_or_else(|e| panic!("{len} bytes, {options:?}: {e}"));
            assert!(back == data, "{len} bytes, {options:?}: decoded bytes differ");
        }
    }
}

/// Megabytes of data that keeps copying from its start, so that offsets need
/// the 4-byte form.
fn distant_copies() -> Vec<u8> {
    let seed = sample(70_000, 99);
    let mut data = Vec::new();
    while data.len() < 13_500_000 {
        data.extend(&seed);
        data.extend(sample(150_000, data.len() as u64));
    }
    data
}

/// Unrelated 16-byte records, then the same records in another order: every
/// record of the second part is its own far match, more than 4095 to a half.
fn shuffled_records() -> Vec<u8> {
    let mut rng = common::Rng(0x5eed);
    let records: Vec<[u8; 16]> = (0..20_000).map(|_| std::array::from_fn(|_| rng.next() as u8)).collect();
    let mut data: Vec<u8> = records.concat();
    for i in 0..records.len() {
        data.extend(records[(i * 7919) % records.len()]);
    }
    data
}

#[test]
fn far_offsets_reach_across_blocks() {
    let data = distant_copies();
    let stream = encode(&data, &Options::default());
    assert!(stream.len() < data.len());
    assert!(decode_to_vec(&stream, data.len()).as_deref() == Ok(&data[..]));
}

#[test]
fn thousands_of_far_matches_in_one_half() {
    let data = shuffled_records();
    for options in
        [Options::default(), Options { chunk: matches(huff6(), huff6()), compact_headers: true, min_match: 4 }]
    {
        let stream = encode(&data, &options);
        assert!(stream.len() < data.len() * 3 / 4);
        assert!(decode_to_vec(&stream, data.len()).as_deref() == Ok(&data[..]));
    }
}

#[test]
fn stored_blocks_of_other_stream_types() {
    let data = sample(300_000, 5);
    for kind in [5, 6, 10, 11, 12] {
        assert!(decode_to_vec(&encode_stored(&data, kind), data.len()).as_deref() == Ok(&data[..]));
    }
    assert_eq!(decode_to_vec(&encode_stored(&data, 7), data.len()), Err(Error::Unsupported("stream type")));
}

#[test]
fn a_block_of_one_byte() {
    let data = vec![0x5a; 262_144 + 40_000];
    let stream = encode(&data, &Options::default());
    assert_eq!(stream.len(), 12);
    assert!(decode_to_vec(&stream, data.len()).as_deref() == Ok(&data[..]));
}

#[test]
fn empty_input_decodes_to_nothing() {
    assert_eq!(decode_to_vec(&[], 0), Ok(vec![]));
    assert_eq!(decode_to_vec(&[0xcc, 0x06], 0), Err(Error::SizeMismatch));
    assert_eq!(decode_to_vec(&[], 1), Err(Error::Truncated));
}

#[test]
fn wrong_length_is_an_error_not_a_panic() {
    let data = sample(10_000, 3);
    for options in variants() {
        let stream = encode(&data, &options);
        for wrong in [0, 1, 9_999, 10_001, 300_000] {
            assert!(decode_to_vec(&stream, wrong).is_err(), "{options:?} decoded to {wrong} bytes");
        }
        for cut in [0, 1, 2, 5, stream.len() / 2, stream.len() - 1] {
            assert!(decode_to_vec(&stream[..cut], data.len()).is_err());
        }
    }
}

/// Writes one stream per variant and size to `$KRAKEN_VECTOR_DIR`, each as a
/// 4-byte little-endian decoded length followed by the stream, for
/// `tools/parity.mjs` to put through the reference decoder. Run with
/// `cargo test --release --test roundtrip -- --ignored export_vectors`.
#[test]
#[ignore = "writes files; needs KRAKEN_VECTOR_DIR"]
fn export_vectors() {
    let dir = std::env::var("KRAKEN_VECTOR_DIR").expect("set KRAKEN_VECTOR_DIR");
    std::fs::create_dir_all(&dir).expect("create the vector folder");
    let mut count = 0;
    for len in [9, 300, 4096, 65_536, 65_537, 131_073, 262_145, 700_000] {
        for seed in 1..=3u64 {
            let data = sample(len, seed * 1000 + len as u64);
            for (index, options) in variants().iter().enumerate() {
                let mut file = (data.len() as u32).to_le_bytes().to_vec();
                file.extend(encode(&data, options));
                std::fs::write(format!("{dir}/{len}-{seed}-{index}.kdv"), file).expect("write a vector");
                count += 1;
            }
        }
    }
    for (name, data) in [
        ("distant", distant_copies()),
        ("shuffled", shuffled_records()),
        ("zeros", vec![0; 600_000]),
        ("stored6", sample(300_000, 6)),
    ] {
        for (index, options) in [
            Options::default(),
            Options { chunk: matches(huff6(), rle(Some(huff6()))), compact_headers: true, min_match: 4 },
        ]
        .iter()
        .enumerate()
        {
            let mut file = (data.len() as u32).to_le_bytes().to_vec();
            file.extend(if name == "stored6" { encode_stored(&data, 6) } else { encode(&data, options) });
            std::fs::write(format!("{dir}/{name}-{index}.kdv"), file).expect("write a vector");
            count += 1;
        }
    }
    println!("{count} vectors written to {dir}");
}

/// Single-feature vectors for narrowing down a disagreement with the
/// reference decoder. Same file format and folder as `export_vectors`.
#[test]
#[ignore = "writes files; needs KRAKEN_VECTOR_DIR"]
fn export_probes() {
    let dir = std::env::var("KRAKEN_VECTOR_DIR").expect("set KRAKEN_VECTOR_DIR");
    std::fs::create_dir_all(&dir).expect("create the vector folder");
    let m = |literals: Coding, commands: Coding, near_split: Option<(Coding, Coding)>, delta_literals: bool| {
        ChunkKind::Matches { literals, commands, near_split, delta_literals }
    };
    let s = || Coding::Stored;
    let h3 = |rice_bits| Coding::Huffman3 { rice_bits };
    let h6 = |rice_bits| Coding::Huffman6 { rice_bits };
    let cases: Vec<(&str, ChunkKind)> = vec![
        ("plain", m(s(), s(), None, false)),
        ("lit-h3k1", m(h3(1), s(), None, false)),
        ("lit-h6k2", m(h6(2), s(), None, false)),
        ("cmd-h3k0", m(s(), h3(0), None, false)),
        ("cmd-h3k1", m(s(), h3(1), None, false)),
        ("cmd-h3k2", m(s(), h3(2), None, false)),
        ("cmd-h3k3", m(s(), h3(3), None, false)),
        ("cmd-h6k0", m(s(), h6(0), None, false)),
        ("cmd-h6k2", m(s(), h6(2), None, false)),
        ("lit-rle", m(rle(None), s(), None, false)),
        ("cmd-rle", m(s(), rle(None), None, false)),
        ("lit-rle-h3", m(rle(Some(h3(1))), s(), None, false)),
        ("lit-rle-h6", m(rle(Some(h6(1))), s(), None, false)),
        ("lit-rle-rle", m(rle(Some(rle(None))), s(), None, false)),
        ("cmd-rle-h6", m(s(), rle(Some(h6(2))), None, false)),
        ("near-ss", m(s(), s(), Some((s(), s())), false)),
        ("near-rle-s", m(s(), s(), Some((rle(None), s())), false)),
        ("near-s-h6", m(s(), s(), Some((s(), h6(2))), false)),
        ("near-h3-h3", m(s(), s(), Some((h3(1), h3(1))), false)),
        ("delta", m(s(), s(), None, true)),
        ("delta-h6", m(h6(2), s(), None, true)),
        ("stored", ChunkKind::Stored),
    ];
    for len in [300, 4096, 70_000, 262_145] {
        let data = sample(len, 77 + len as u64);
        for (name, chunk) in &cases {
            for compact_headers in [false, true] {
                let options = Options { chunk: chunk.clone(), compact_headers, min_match: 4 };
                let mut file = (data.len() as u32).to_le_bytes().to_vec();
                file.extend(encode(&data, &options));
                let tag = if compact_headers { "c" } else { "n" };
                std::fs::write(format!("{dir}/probe.{name}.{tag}.{len}.kdv"), file).expect("write a vector");
            }
        }
    }
}
