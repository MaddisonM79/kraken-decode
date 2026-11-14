//! Any bytes, any claimed length: the decoder must return, never panic.
//!
//! The first three bytes of the input choose the output length (up to a little
//! over one block, so both the two-chunk and the next-block paths are in
//! reach); the rest is the stream.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|input: &[u8]| {
    let Some((head, stream)) = input.split_first_chunk::<3>() else { return };
    let len = (usize::from(head[0]) | usize::from(head[1]) << 8 | usize::from(head[2]) << 16) % 300_000;
    let mut out = vec![0u8; len];
    let _ = kraken_decode::decode(stream, &mut out);
});
