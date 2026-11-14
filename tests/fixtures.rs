//! The committed fixtures.
//!
//! `tests/fixtures/player.sav` is the scrubbed player save from the Palworld
//! core of Ryna (MIT, same author): a real save with every name and
//! world-made id replaced, wrapped in stored blocks. Nothing here comes from
//! an unscrubbed save.
//!
//! `tests/fixtures/player.*.kdv` are that fixture's decoded bytes compressed
//! again by this repository's test encoder (`tests/common`), one file per way
//! of storing a chunk: a 4-byte little-endian decoded length, then the
//! stream. They are committed so that a change to the test encoder cannot
//! quietly change what the decoder is checked against, and so that the
//! JavaScript test (`js/test.mjs`) has streams to decode. Regenerate with
//! `cargo test --test fixtures -- --ignored write_fixtures`.

mod common;

use common::{encode, ChunkKind, Coding, Options};
use kraken_decode::{decode, decode_to_vec};

const PLAYER_SAV: &[u8] = include_bytes!("fixtures/player.sav");

/// A `PlM` save: decoded length, compressed length, magic, type byte, body.
fn save_body(save: &[u8]) -> (&[u8], usize) {
    let field = |at: usize| u32::from_le_bytes(save[at..at + 4].try_into().expect("four bytes")) as usize;
    assert_eq!(&save[8..11], b"PlM");
    (&save[12..12 + field(4)], field(0))
}

fn player_gvas() -> Vec<u8> {
    let (body, len) = save_body(PLAYER_SAV);
    decode_to_vec(body, len).expect("the stored fixture decodes")
}

fn variants() -> Vec<(&'static str, Options)> {
    let h3 = || Coding::Huffman3 { rice_bits: 1 };
    let h6 = || Coding::Huffman6 { rice_bits: 2 };
    let rle = |inner: Option<Coding>| Coding::RunLength { literals: inner.map(Box::new) };
    let matches = |literals, commands, near_split, delta_literals, compact_headers| Options {
        chunk: ChunkKind::Matches { literals, commands, near_split, delta_literals },
        compact_headers,
        min_match: 4,
    };
    vec![
        ("plain", matches(Coding::Stored, Coding::Stored, None, false, false)),
        ("huffman", matches(h6(), h6(), Some((h3(), h6())), false, false)),
        ("runs", matches(rle(Some(h3())), rle(None), None, true, true)),
        ("array", Options { chunk: ChunkKind::Array(rle(Some(h6()))), compact_headers: false, min_match: 4 }),
    ]
}

#[test]
fn the_scrubbed_player_save_decodes() {
    let gvas = player_gvas();
    assert_eq!(gvas.len(), 27_462);
    assert_eq!(&gvas[..4], b"GVAS");
    // The scrubbed host id (…0001) is in there, as UTF-16 hex or as bytes.
    assert!(gvas.windows(4).any(|w| w == b"Pal."));
}

#[test]
fn the_committed_streams_decode_to_the_player_save() {
    let gvas = player_gvas();
    for (name, _) in variants() {
        let path = format!("{}/tests/fixtures/player.{name}.kdv", env!("CARGO_MANIFEST_DIR"));
        let file = std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
        let len = u32::from_le_bytes(file[..4].try_into().expect("four bytes")) as usize;
        let mut out = vec![0u8; len];
        assert_eq!(decode(&file[4..], &mut out), Ok(()), "{name}");
        assert!(out == gvas, "{name}: decoded bytes differ");
        assert!(file.len() < gvas.len(), "{name} did not compress");
    }
}

#[test]
fn the_test_encoder_still_writes_the_committed_streams() {
    let gvas = player_gvas();
    for (name, options) in variants() {
        let path = format!("{}/tests/fixtures/player.{name}.kdv", env!("CARGO_MANIFEST_DIR"));
        let mut file = (gvas.len() as u32).to_le_bytes().to_vec();
        file.extend(encode(&gvas, &options));
        assert!(
            std::fs::read(&path).ok().as_deref() == Some(&file[..]),
            "{name}: run write_fixtures, and check the new files against the reference decoder"
        );
    }
}

#[test]
#[ignore = "rewrites tests/fixtures/player.*.kdv"]
fn write_fixtures() {
    let gvas = player_gvas();
    for (name, options) in variants() {
        let mut file = (gvas.len() as u32).to_le_bytes().to_vec();
        file.extend(encode(&gvas, &options));
        std::fs::write(format!("{}/tests/fixtures/player.{name}.kdv", env!("CARGO_MANIFEST_DIR")), file)
            .expect("write a fixture");
    }
}
