# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.2] - 2026-11-15

No change to the decoder or its API. This is the first version the release
workflow published to crates.io itself, through trusted publishing; 0.1.1 was
uploaded by hand.

## [0.1.1] - 2026-11-14

The first published version. 0.1.0 was a GitHub release only, and was withdrawn
the same day: the same decoder, with the crate package put right (it now
carries `docs/`, and no longer the repository's agent notes).

### Added

- `decode` and `decode_to_vec`: decode a stream of type 10 (the byte-oriented
  member of the Kraken family, as written by Palworld's Mac build) into a
  buffer of known length. Covers stored blocks, one-byte blocks, stored
  chunks, chunks of matches and literals in both literal modes, chunks that
  are a single coded array, and arrays that are stored, Huffman coded (three
  or six streams) or run-length coded, nested or not.
- `Error`: `Truncated`, `Corrupt`, `Unsupported`, `SizeMismatch`. The decoder
  returns one of these on any input it cannot decode; it does not panic.
- A WebAssembly build (`wasm/`) with a hand-written JavaScript loader
  (`js/`), exporting `decompress(data, rawSize)`.
- `docs/FORMAT.md`, a description of the format as far as this crate decodes
  it, and `docs/SOURCES.md`, where every rule in it comes from.
