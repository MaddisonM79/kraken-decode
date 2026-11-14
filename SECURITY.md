# Security Policy

## Supported Versions

`kraken-decode` is a pre-1.0 crate. Only the latest published `0.x` minor release receives security fixes. There is no LTS line.

| Version | Supported          |
| ------- | ------------------ |
| 0.1.x   | :white_check_mark: |

## Reporting a Vulnerability

**Please do not report security vulnerabilities through public GitHub issues, pull requests, or discussions.**

Use GitHub's private vulnerability reporting:

1. Go to the [Security tab](https://github.com/MaddisonM79/kraken-decode/security/advisories) of the repository.
2. Click **Report a vulnerability**.
3. Fill in the form with as much detail as you can: affected versions, the input that triggers the problem (or how to build it), expected and observed behavior. If the input is a real save file, scrub it or describe it instead of attaching it.

You can expect:

- An initial acknowledgement within **7 days** of submission.
- A status update at least every **14 days** while the issue is being investigated.
- A coordinated disclosure timeline negotiated together once the impact is understood.

## Scope

This crate parses bytes from files other people can hand you, so its contract is strict. In scope:

- Any panic reachable from `decode` or `decode_to_vec` on any input. The decoder is contractually panic-free: the library forbids `unsafe`, denies indexing that could panic, and is fuzzed.
- Unbounded CPU or memory use from a short input. `decode` allocates scratch space bounded by the chunk size (a few hundred kilobytes) whatever the input says; `decode_to_vec` allocates the length its caller passes, and nothing more.
- A read or write outside the buffers passed in, through the WebAssembly exports in `wasm/` when they are called as documented.
- Wrong output: a stream the reference decoder accepts that this crate decodes to different bytes without an error.

Out of scope:

- Streams this crate reports as `Unsupported`. The README lists what is covered.
- Accepting a damaged stream that happens to decode. The format carries no checksum that this crate reads; integrity is the container's job.
- Vulnerabilities in the test-only dependencies (please report to the upstream crate).

## Security Best Practices for Consumers

- The decoded length comes from whatever wraps the stream (Palworld's save header, for one). Check it against a limit of your own before calling `decode_to_vec`.
- Treat the decoded bytes as untrusted too: a valid stream can still hold a malformed save.
