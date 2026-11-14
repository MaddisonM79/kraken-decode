# Contributing to kraken-decode

Thanks for taking the time to contribute! This document describes the one rule that defines the project, then the workflow for issues and pull requests and the local tooling. How the format works is in [`docs/FORMAT.md`](docs/FORMAT.md); where each rule in it comes from is in [`docs/SOURCES.md`](docs/SOURCES.md).

## The clean-room rule

This crate exists so that software under the MIT licence can read these streams without taking on the GPL. The best-known open-source decoders of this format family (`ooz` by powzix, its forks, and the ports and bindings built on it, such as `ooz-wasm`, `libooz`, `oozextract` and `oozle`) are GPL. A translation of GPL source is still GPL, whatever licence file sits beside it. So:

- **Do not open, read, search or quote the source of those projects** to work on this one: not in `node_modules`, not on a code host, not in a registry's source browser, not through a search engine's snippet, not through an AI assistant asked to look for you. If you have studied that source before, please don't contribute decoder logic here; tests, docs, tooling and fuzzing are all welcome.
- **Write from three things only:**
  1. public prose about the format, with its licence checked and the source added to `docs/SOURCES.md`;
  2. the bytes themselves;
  3. a reference decoder used strictly as a **black box**: call its compiled decode function on an input, compare the output. `tools/parity.mjs` does exactly that and nothing else.
- **Code under a permissive licence** may be used if its origin is clear: say which code, under which licence, and credit it in `docs/SOURCES.md`. If its origin is unclear (for example a decoder that says it follows a specification it does not name), do not use it and do not read it.
- **If you cannot get further without looking at GPL source, stop** and open an issue saying what is missing. A decoder that refuses a stream is fine. A decoder with borrowed code is not.
- Every new rule in `docs/FORMAT.md` needs a line in `docs/SOURCES.md` saying how it was established (which prose, which experiment). A pull request that makes the decoder accept more without that will not be merged.
- The decoder accepts only what has been confirmed. A part of the format nobody has verified is refused with `Error::Unsupported`, not guessed at.

The same goes for other proprietary or copyleft implementations of the format, including the vendor's own library: no disassembly, no decompilation, no reading of leaked source.

## No real saves in the repository

A save file is one person's world. Never commit bytes from a real save, compressed or decoded, and never attach one to an issue.

- `tests/fixtures/player.sav` is a **scrubbed** save (every name and world-made id replaced), taken from the Palworld core of [Ryna](https://github.com/MaddisonM79/ryna_agent), where `scripts/scrub.ts` makes it.
- `tests/fixtures/player.*.kdv` are that scrubbed data compressed by this repository's own test encoder.
- Everything else in the tests is generated (`tests/common`).
- `.gitignore` refuses `*.sav` and `*.kdv` outside `tests/fixtures`. Keep real saves for parity runs in a folder outside the repository.

## Reporting Bugs

- Search [open and closed issues](https://github.com/MaddisonM79/kraken-decode/issues?q=is%3Aissue) first.
- Open a new issue with: the crate version, your Rust toolchain (`rustc --version`), what you called, the error you got (`Error` prints which part of the stream it refused), and what you expected. For a stream that decodes wrongly or is refused, say where the stream comes from (game and version) rather than attaching a save; a maintainer will ask for a scrubbed or generated reproduction.
- For security issues (a panic, a hang, runaway memory), please follow [`SECURITY.md`](SECURITY.md) instead of opening a public issue.

## Pull Requests

### Before you push

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test
cargo build --no-default-features --target thumbv7em-none-eabihf   # no_std
cargo build -p kraken-decode-wasm --target wasm32-unknown-unknown --release
node js/build.mjs && node --test js/test.mjs
```

If your change touches the public API or what the decoder accepts, update `CHANGELOG.md` under `[Unreleased]`, following the [Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/) format already established there, and `docs/FORMAT.md`.

### Parity

There are two layers, and a change to the decoder should pass both.

**Public, in CI.** `tests/roundtrip.rs` encodes generated data with the test encoder in `tests/common` in every combination of array types it knows, and decodes it back. `tests/fixtures.rs` does the same for the scrubbed save.

**Private, on your machine.** `tools/parity.mjs` decodes every save in a folder with this crate's WebAssembly build and with a reference decoder, and compares the bytes:

```sh
cargo build -p kraken-decode-wasm --target wasm32-unknown-unknown --release && node js/build.mjs
KRAKEN_PARITY_DIR=/path/outside/the/repo/saves \
KRAKEN_PARITY_ORACLE=/path/to/reference/index.js \
node tools/parity.mjs
```

The reference is any module exporting `decompress(data, rawSize)`. It is a black box: point the script at its entry file and do not open it. Neither the saves nor the reference belong in this repository, in `package.json`, or in CI.

A round trip through our own encoder only proves the two halves agree with each other. What makes it meaningful is that the encoder's output was also put through the reference:

```sh
KRAKEN_VECTOR_DIR=/tmp/kd-vectors cargo test --release --test roundtrip -- --ignored export
KRAKEN_PARITY_DIR=/tmp/kd-vectors KRAKEN_PARITY_ORACLE=… node tools/parity.mjs
```

Do this whenever you change `tests/common`, and regenerate the committed streams (`cargo test --test fixtures -- --ignored write_fixtures`) only after it passes.

### Fuzzing

The decoder must return on every input: no panic, no hang, no unbounded allocation.

```sh
cargo test --test no_panic                         # stable, runs in CI
KRAKEN_FUZZ_ITERS=1000000 cargo test --test no_panic
cargo +nightly fuzz run decode                     # coverage-guided; needs cargo-fuzz
```

A crash found by the fuzzer becomes a test in `tests/no_panic.rs` before it is fixed.

### Commit messages

Conventional Commits are encouraged but not enforced. A typical prefix set:

- `feat:` new public API, or a part of the format newly decoded
- `fix:` bug fix
- `perf:` performance improvement
- `refactor:` internal change, no behavior diff
- `test:` test-only change
- `docs:` documentation
- `chore:` build / CI / tooling

Co-author trailers from AI tools (e.g., `Co-Authored-By: Claude`) **should not** be added to commits or PR descriptions.

### What gets reviewed

- **Provenance**: see the clean-room rule. This is the first thing checked.
- **No panics on input**: the library is `#![forbid(unsafe_code)]` and denies `clippy::indexing_slicing`, `unwrap_used`, `expect_used` and `panic`. Every read is bounds-checked and every failure is an `Error`. Arithmetic on values from the stream must not overflow (the debug-build fuzz runs catch this).
- **Bounded work**: nothing may allocate or loop in proportion to a number read from the stream without checking it against the chunk's own size first.
- **No new `unsafe`**. The only `unsafe` in the repository is the pointer handling in `wasm/src/lib.rs`, and each block there says what it relies on.
- **API surface**: new public items need rustdoc with an example. The crate is `#![warn(missing_docs)]`.

## License

By contributing, you agree that your contributions are licensed under the [MIT License](LICENSE) used by this project, and you confirm that they follow the clean-room rule above.
