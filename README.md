# kraken-decode

[![crates.io](https://img.shields.io/crates/v/kraken-decode.svg)](https://crates.io/crates/kraken-decode)
[![docs.rs](https://img.shields.io/docsrs/kraken-decode)](https://docs.rs/kraken-decode)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![MSRV](https://img.shields.io/badge/MSRV-1.87-blue.svg)](https://www.rust-lang.org)

A decoder for the compressed data inside Palworld's `PlM` save files, written from public descriptions and from the bytes themselves, so that MIT-licensed software can read those saves without a GPL dependency.

It only decodes. It has no dependencies, no `unsafe`, works without `std`, and returns an error instead of panicking on any input. It also builds to a 37 KB WebAssembly module with a hand-written JavaScript loader.

## What it reads

Palworld's current builds compress their saves with a commercial library, in a format family usually called by the name of its first member, Kraken. The saves use the family's byte-oriented stream type (type 10, which the format's users call Mermaid). This crate is **compatible with that stream type as Palworld writes it**:

- stored blocks, blocks of one repeated byte, stored chunks;
- chunks of matches and literals, in both literal modes, with offsets reaching back to the start of the stream;
- chunks that are one coded array;
- arrays stored as they are, Huffman coded (three or six streams) or run-length coded, including run-length arrays whose literals are themselves coded.

It also reads stored (uncompressed) blocks of the family's other stream types, which is what scrubbed test fixtures are usually wrapped in.

It does **not** decode the other members of the family (Kraken itself, Leviathan and the rest) or the parts of the format that Palworld's saves do not use; those come back as `Error::Unsupported`. [`docs/FORMAT.md`](docs/FORMAT.md) describes exactly what is covered and lists what is not.

This project is not affiliated with or endorsed by Epic Games Tools, whose Oodle library defines the format, or by Pocketpair. Product names are used only to say what the crate is compatible with.

## How it was written, and checked

The well-known open-source decoders for this format are GPL, and a port of GPL code stays GPL. This crate is not a port of anything. Its sources are a few public articles on the format's design, the bytes of real saves, and a reference decoder used strictly as a black box: fed inputs, its output compared, its source never read. [`docs/SOURCES.md`](docs/SOURCES.md) lists every source and says how each rule of the format was established, including a plain statement of what "clean-room" can and cannot mean here.

- **Against real saves:** every one of 7,745 real Palworld save files (61 GB of decoded data, from 1,549 saves of one world over a month of play) decodes to exactly the bytes the reference decoder produces. Those saves are private and are not in this repository.
- **Against generated data:** a test-only encoder in this repository writes streams in every combination of array types it knows; the decoder reads them back, and the reference decoder agrees on all of them. These tests are public and run in CI.
- **Against damage:** mutated and random inputs, in debug builds with overflow checks on and under a coverage-guided fuzzer, have not produced a panic.

## Quick start

```sh
cargo add kraken-decode
```

The format does not record how long the decoded data is; the container around it does. A Palworld `.sav` starts with the decoded length, the compressed length, the magic `PlM` and a type byte, then the stream:

```rust
use kraken_decode::{decode_to_vec, Error};

fn read_plm(save: &[u8]) -> Result<Vec<u8>, Error> {
    let field = |at: usize| -> Option<usize> {
        let bytes: [u8; 4] = save.get(at..at + 4)?.try_into().ok()?;
        Some(u32::from_le_bytes(bytes) as usize)
    };
    let (Some(decoded_len), Some(stream_len)) = (field(0), field(4)) else { return Err(Error::Truncated) };
    if save.get(8..11) != Some(b"PlM".as_slice()) {
        return Err(Error::Corrupt("not a PlM save"));
    }
    // Your own limit on what you are willing to allocate.
    if decoded_len > 1 << 30 {
        return Err(Error::Corrupt("decoded length is implausible"));
    }
    let stream = save.get(12..12 + stream_len).ok_or(Error::Truncated)?;
    decode_to_vec(stream, decoded_len)
}

// A tiny save: four bytes in one stored block.
let save = [4, 0, 0, 0, 6, 0, 0, 0, b'P', b'l', b'M', 0x31, 0xcc, 0x06, b'G', b'V', b'A', b'S'];
assert_eq!(read_plm(&save).as_deref(), Ok(b"GVAS".as_slice()));
```

`decode(src, dst)` does the same into a buffer you provide, which is the form to use without an allocator-backed `Vec` of your own.

Or, to pin manually instead of `cargo add`:

```toml
[dependencies]
kraken-decode = "0.1"
```

## Errors

Every failure is an [`Error`](https://docs.rs/kraken-decode/latest/kraken_decode/enum.Error.html):

| Variant | Meaning |
|---------|---------|
| `Truncated` | The input ended early. A save read while the game was still writing it usually lands here. |
| `Corrupt(part)` | The input contradicts itself; `part` names where. |
| `Unsupported(part)` | A part of the format this crate does not cover. |
| `SizeMismatch` | The stream does not decode to exactly the length given, or bytes were left over. |

The format has no checksum that this crate reads, so a damaged stream can occasionally decode to wrong bytes without an error. If that matters, check the result (a Palworld save starts with `GVAS`).

## `no_std`

The crate is `#![no_std]` and needs only `alloc`, for scratch buffers bounded by the format's chunk size (a few hundred kilobytes at most, whatever the input claims).

## WebAssembly and JavaScript

`wasm/` builds the decoder for `wasm32-unknown-unknown` with three exported functions and no bindings generator. `js/src/loader.mjs` is the entire JavaScript side, and `js/build.mjs` writes a single module with the WebAssembly embedded:

```sh
cargo build -p kraken-decode-wasm --target wasm32-unknown-unknown --release
node js/build.mjs          # writes js/dist/kraken-decode.mjs
```

```js
import { decompress } from "./js/dist/kraken-decode.mjs";

// data: the compressed stream; rawSize: the decoded length from the container.
const raw = decompress(data, rawSize); // a new Uint8Array; throws KrakenDecodeError
```

Releases attach `kraken-decode.mjs`, its type declarations and the bare `.wasm`, with checksums.

## Performance

Speed was not the goal; not being able to crash was. On the saves above, through WebAssembly in Node, it takes roughly twice as long as the reference decoder: about 27 seconds for the 61 GB above on an M-series Mac, against 13.

## Contributing

Please read [`CONTRIBUTING.md`](CONTRIBUTING.md) first. One rule there matters more than the rest: nobody who works on this crate may read the GPL decoders' source to do it.

## Acknowledgements

Every rule in this decoder was checked against `ooz-wasm` (GPL-3.0-or-later), a WebAssembly build of powzix's `ooz`, run as a black box. Its source was not read and none of its code is here, but this crate could not have been verified without it.

Thanks also to Fabian Giesen for his public articles on the format's design.

## License

[MIT](https://github.com/MaddisonM79/kraken-decode/blob/main/LICENSE).
