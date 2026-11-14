# Sources

Where the knowledge in this crate comes from, so that anyone can check the claim it makes: that it
was written without reading another decoder's source.

There are well-known open-source decoders for this format family under the GPL (`ooz` and its
ports and bindings). **None of their source was opened, read, searched or quoted** in making this
crate: not in a package folder, not on a code host, not in a registry's source view. The rule that
keeps it that way is in [`CONTRIBUTING.md`](../CONTRIBUTING.md).

## What was used

### 1. Public prose

| Source | Licence | Used for |
|--------|---------|----------|
| Fabian Giesen, "Entropy coding in Oodle Data: the big picture" (2021-07-09), <https://fgiesen.wordpress.com/2021/07/09/entropy-coding-in-oodle-data-the-big-picture/> | None stated. Read for facts; no text or code copied. | Data is processed in 128 KiB chunks with explicitly signalled sizes; a chunk is several independent byte streams, each of which may be stored as is; entropy decoding is a separate pass per stream. |
| Fabian Giesen, "Entropy coding in Oodle Data: Huffman coding" (2021-08-30), <https://fgiesen.wordpress.com/2021/08/30/entropy-coding-in-oodle-data-huffman-coding/> | As above. | Huffman codes are limited to 11 bits; canonical codes are ordered by length and then by symbol value; a complete code satisfies the Kraft equality; the fast decoders read least significant bit first from a 2048-entry table; there is a 3-stream and a 6-stream layout. |
| Fabian Giesen, "Entropy decoding in Oodle Data: Huffman decoding on the Jaguar" (2022-04-04), <https://fgiesen.wordpress.com/2022/04/04/entropy-decoding-in-oodle-data-huffman-decoding-on-the-jaguar/> | As above. | The physical layout of three streams: the first forward, the third forward after it, the second backward from the end, with a single stored size. |
| Fabian Giesen, "Entropy decoding in Oodle Data: x86-64 6-stream Huffman decoders" (2023-10-29), <https://fgiesen.wordpress.com/2023/10/29/entropy-decoding-in-oodle-data-x86-64-6-stream-huffman-decoders/> | As above. | Six streams are two independent groups of three, one for each half of the output. |
| Fabian Giesen, "Oodle, Kraken etc. misconceptions" (2024-08-08), <https://fgiesen.wordpress.com/2024/08/08/oodle-kraken-etc-misconceptions/> | As above. | Background: how the members of the family relate. |
| Charles Bloom's blog, posts of 2016 and 2017 on Kraken, Mermaid and Selkie, <http://cbloomrants.blogspot.com/> | None stated. | Background only: no layout detail is in them. |
| A community mod page for Palworld save tools (Nexus Mods, Palworld mod 5441) | n/a | Only the naming: that Palworld's saves use the family's stream types and that type 6 is called Kraken. |

These posts give the outline. None of them gives a header layout, how a Huffman code is
transmitted, the run-length scheme, or anything about the match layer.

### 2. Code under a permissive licence

| Source | Licence | Used for |
|--------|---------|----------|
| The Palworld core of [Ryna](https://github.com/MaddisonM79/ryna_agent) (`cores/palworld/src/sav.ts`, `scripts/scrub.ts`, the scrubbed fixtures) | MIT, same author as this crate | The `PlM` container (two lengths, the magic, a type byte), the stored-block header `0xCC 0x06`, the 256 KiB block size, and the scrubbed fixture in `tests/fixtures`. |

### 3. Bytes

The compressed bodies of real Palworld `PlM` saves belonging to the author, read in a private
folder outside any repository. None of those bytes are in this repository. What can be learned by
looking: where the text of a save sits in the compressed stream, which sizes add up to which
lengths, which bytes repeat.

### 4. A reference decoder, as a black box

`ooz-wasm` 2.0.0 (npm, GPL-3.0-or-later) was called through its one exported function,
`decompress(data, rawSize)`, and its output compared. Of the package, only `index.d.ts` (the type
declarations of that function) and `package.json` were read. Its JavaScript and WebAssembly were
executed, never read.

A decoder used this way answers two kinds of question:

- **What does this input decode to?** Any array from a real save can be wrapped as a chunk of its
  own and decoded alone, which gives the plain bytes of that array.
- **Is this input valid, and what does it mean?** An input built by hand, or a real one with one
  byte changed, is either refused or decodes to something, and the difference shows what the
  changed part does.

### Not used

- The forum threads on encode.su about open-source decoders of this family: the fetch was refused
  and they were never read.
- `lztans` (crates.io, MIT OR Apache-2.0), a decoder for the same family that says it was written
  from a prose specification it does not name. Its provenance could not be checked, so its source
  was not opened.
- Any source file of `ooz`, `libooz`, `ooz-wasm`, `oozextract`, `oozle` or another port.

## How each rule was established

"Samples" are the private real saves. "Probe" means an input built by hand and given to the
reference. "Vectors" are streams made by this repository's test encoder (`tests/common`) and given
to both decoders (`tools/parity.mjs`).

| Rule (section of `FORMAT.md`) | Established by |
|-------------------------------|----------------|
| Blocks of 256 KiB, each with a 2-byte header (1) | Walking every sample: the headers land exactly on the end of each file only with this size. The Palworld core's scrub script writes the same. |
| Header byte 0: `0x0C`/`0x8C` compressed, `0x4C`/`0xCC` stored; byte 1: types 5, 6, 10, 11, 12 (1) | Probe: every value of each byte tried against a real compressed stream and against stored data. |
| Group word: 18-bit size less one, high bits ignored (2) | Samples (sizes add up); probe over all values of the high bits. |
| One byte repeated (2) | Probe: size field `0x3FFFF` with the next bit and one byte gave a block of that byte. |
| A group as long as its block is stored; a full block is never a stored group (2) | Probe, and vectors (the reference refused a full-size stored group). |
| Chunks of 128 KiB (3) | Giesen; confirmed by walking the samples. |
| Chunk word: top bit, 4-bit mode, 19-bit size; equal size means stored, with mode 0 (3) | Samples; probe over the top five bits; probe with a stored second chunk. |
| A chunk can be a single coded array (3) | The smallest sample files are exactly this. Decoding matched. |
| Array headers, long and short (4) | Samples: the two length fields of the 5-byte form equal the known decoded and coded lengths. The short forms by probe (nested inside a run-length array) and by vectors. |
| A coded array is shorter than its data (4) | Vectors: the reference refused every array that was not. |
| Huffman: 11-bit canonical codes, bit order, three streams and their layout, six as two groups (4.1) | Giesen. Confirmed on real arrays: the reference's output for an array, against this crate's. The 16-bit and 24-bit size fields and the split of the halves were found by trying the few possibilities on real arrays. |
| The code description (4.1) | Real arrays. The header fields were read off by comparing descriptions of arrays whose plain bytes were known (the count field equals the number of distinct bytes, less one). The span count, spans and extra bits were fitted to arrays whose set of byte values was known. The length steps were fitted by requiring a complete code: of all candidate conventions, the ones that give a Kraft sum of exactly one on every array tried are equivalent to the one described. The whole description then decoded every Huffman array in the samples. |
| Run-length arrays (4.2) | One byte of a real array changed at a time: each literal changes exactly one output byte, which gives the literal runs and the gaps between them; the command bytes were fitted to those. The long forms and the fill-byte command by probe. The two layouts by samples. |
| A one-byte run-length payload is that byte repeated (4.2) | Samples: found when the full set of saves was first compared, in arrays the first few dozen saves did not contain. Then probe, over several byte values and lengths. |
| Offsets of 8 and up; overlapping copies repeat (5) | Vectors: with a smaller offset the reference's output is not a copy. Samples never use one. |
| The parts of a chunk and their order (6) | Samples whose arrays are all stored, where every part is visible, then every sample. |
| The 8 raw bytes at the start of a stream (6) | Samples: every first chunk starts with the first 8 bytes of the save. |
| Commands, long lengths, near and far offsets, the two halves (6) | Fitted on stored-array chunks against the reference's output, starting from guesses and correcting them: two guesses about which command takes which kind of offset, and the 4-byte far offset rule, were wrong at first and fixed by the mismatches. Then every sample. |
| Far offset counts of 4095 (6) | Vectors only. |
| Literal mode 0 (6) | One sample chunk, and vectors. |

## What the checks cover

- Every chunk of every sample decodes to the reference's bytes (the count is in the README, and
  grows as saves are added).
- Every stream the test encoder makes, in every combination of array types it knows, decodes to
  the same bytes in both decoders. That covers paths the samples reach rarely or never.
- Nothing else is claimed. Parts of the format that neither the samples nor the vectors reach are
  refused (`FORMAT.md`, "What is not covered").

## A plain statement about independence

This decoder was written by Nyx, an AI model (Claude, made by Anthropic) working for MaddisonM79,
and that matters to the claim above.

A classic clean room has two teams: one that has seen the original and writes a specification,
and one that has not and writes the code. An AI model cannot be the second team in the strict
sense. It was trained on a great deal of public text and code, it cannot list what that included,
and so it cannot rule out that the GPL decoders were part of it. The model did come to this work
with some idea of how formats of this family are built, and it cannot say where each part of that
idea came from.

What can be said, and checked:

- No GPL source was consulted during the work. The sources are the ones listed above.
- Recollection was used only to choose what to test, never as a fact. Every rule in `FORMAT.md`
  was confirmed against bytes and the black box before any code relied on it, and several
  recollections were wrong and were corrected that way.
- The code was written fresh, in its own structure and words, in a different language from those
  decoders, from `FORMAT.md` and the experiments. It forbids `unsafe`, which shapes it differently
  from a pointer-based decoder.
- The facts of a file format are not anyone's property; the expression of a program is. This
  crate takes the first and none of the second.

Whether that standard is enough for a given use is for the person using the crate to decide. It
is stated here so that the decision can be an informed one.
