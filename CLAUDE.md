# kraken-decode: a clean-room decoder for the streams in Palworld's Mac saves

A Rust crate (MIT, public) that decodes the block-compressed data inside Palworld `PlM` saves:
stream type 10 of the Kraken family (the one its users call "Mermaid"), plus stored blocks of the
other types. Decode only, `no_std` with `alloc`, no `unsafe` in the library, no dependencies.
`wasm/` is the WebAssembly build and `js/` its hand-written loader; the Palworld core of Ryna
(`~/github/MaddisonM79/ryna_agent/cores/palworld`) is the consumer.

## The rule that defines this repository: clean-room

`ooz` (powzix/ooz, its forks, `ooz-wasm`, `libooz`, and ports such as `oozextract` and `oozle`) is
GPL. A translation of GPL source is GPL whatever licence file sits beside it, and that would
defeat this crate's only reason to exist.

- **Never open, read, search or quote that source**: not in `node_modules`, not on GitHub, not in
  a crates.io or docs.rs source view, not through a web search snippet. Never ask a subagent to.
  Block `github.com` and `docs.rs` in searches about the format, and tell any fetch agent the rule.
- Work only from (a) public prose, each source's licence checked and listed in `docs/SOURCES.md`;
  (b) the bytes; (c) `ooz-wasm` strictly as a black box: call its compiled `decompress`, compare
  outputs. Of that package, only `index.d.ts` and `package.json` may be read.
- Recollection of how the format works is a hypothesis to test, never a fact to code from. A rule
  goes into `docs/FORMAT.md` only once an experiment or a cited source confirms it, with a line in
  `docs/SOURCES.md` saying which.
- Code under a permissive licence may be used when its origin is clear: say which and credit it.
  If the origin is unclear (`lztans` is the known case), don't use it and don't read it.
- **If you can't get further without reading GPL source, stop and tell Maddison.** Refusing a
  stream (`Error::Unsupported`) is always an acceptable outcome; borrowed code never is.
- The decoder accepts only what has been confirmed against the reference. Unverified parts of the
  format are refused, not guessed.

`CONTRIBUTING.md` says the same to human contributors. Keep the two in step.

## Real saves never enter the repository

- Maddison's real saves are private. For parity they live outside every repository, in
  `~/.local/share/kraken-decode/samples` (fetched from R2 `palworld-saves` with her go-ahead on
  2026-11-14). The game install and `~/Library/Containers/com.pocketpair.palworld.mac` are not to
  be read.
- Committed binaries are only `tests/fixtures/player.sav` (the scrubbed fixture from the Palworld
  core) and `tests/fixtures/player.*.kdv` (that data re-compressed by the test encoder).
  `.gitignore` refuses any other `*.sav` or `*.kdv`. Don't paste save bytes into docs, tests,
  commit messages or issues either.

## Layout

- `src/decode.rs`: blocks, groups, chunks, and the two public functions.
- `src/lz.rs`: a chunk's matches and literals. `src/array.rs`: array headers and run-length
  arrays. `src/huffman.rs`: code descriptions and the 3/6-stream decode. `src/bits.rs`: bit readers.
- `tests/common/mod.rs`: a test-only **encoder**. It exists because the tests need valid streams
  and may not use real ones. `tests/roundtrip.rs`, `tests/fixtures.rs`, `tests/no_panic.rs`.
- `tools/parity.mjs`: the black-box comparison. `fuzz/`: the cargo-fuzz target (own workspace).
- `docs/FORMAT.md`: the format as decoded. `docs/SOURCES.md`: provenance, rule by rule.

## Commands

```bash
cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test
cargo build --no-default-features --target thumbv7em-none-eabihf
cargo build -p kraken-decode-wasm --target wasm32-unknown-unknown --release && node js/build.mjs && node --test js/test.mjs

# Private parity (needs the samples and the reference; neither is in the repo):
KRAKEN_PARITY_DIR=~/.local/share/kraken-decode/samples \
KRAKEN_PARITY_ORACLE=~/github/MaddisonM79/ryna_agent/cores/palworld/node_modules/ooz-wasm/index.js \
node tools/parity.mjs

KRAKEN_FUZZ_ITERS=1000000 cargo test --test no_panic     # debug build: overflow checks on
cargo +nightly fuzz run decode -- -max_total_time=300
```

- After changing `tests/common`, export its vectors and run them through the reference before
  trusting a green round trip (`CONTRIBUTING.md`, "Parity"). That check has already caught three
  format rules the real saves alone did not show.
- Run parity on the full sample folder before any release.
- Once the Palworld core no longer installs `ooz-wasm`, the oracle path above goes away: keep a
  copy of the package outside every repository for parity, never as a dependency of anything.

## House rules

- The library stays `#![forbid(unsafe_code)]` with `clippy::indexing_slicing`, `unwrap_used`,
  `expect_used` and `panic` denied. The only `unsafe` is the FFI in `wasm/src/lib.rs`.
- No runtime dependencies. `deny.toml` bans the GPL decoders by name.
- Every `uses:` in the workflows is pinned to a commit SHA with its tag in a comment.
- Commits are signed through 1Password; if signing fails, stop and tell Maddison. Push straight to
  `main`. No AI attribution in commits or PRs. A release (a `v*` tag) needs her word.
  The tag's workflow makes the GitHub release (the wasm and loader files); its crates.io job is
  off until the repository variable `CRATES_IO_TRUSTED_PUBLISHING` is `true`, which waits on
  Maddison publishing the first version by hand and adding the trusted publisher.
- The name avoids "Oodle" (Epic's trademark). Docs may say what the crate is compatible with,
  descriptively, and that it is not affiliated.

<!-- global: v2, reviewed 2026-11-14 -->
