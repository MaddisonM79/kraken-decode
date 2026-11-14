# The stream format, as far as this crate decodes it

This describes the compressed data inside a Palworld `PlM` save file: the byte-oriented member of
the Kraken family of formats (stream type 10, which the format's users call "Mermaid"). It covers
what the decoder in `src/` does and no more. Where every rule comes from is in
[`SOURCES.md`](SOURCES.md); the short version is that the outline comes from public prose and the
detail from experiments on bytes, with a reference decoder used as a black box.

It is not a specification of the whole family. Parts that were never seen in a save and could not
be confirmed are listed under [What is not covered](#what-is-not-covered), and the decoder refuses
them.

Numbers are unsigned. "Big-endian" and "little-endian" say which byte of a multi-byte number comes
first. The decoded length of a stream is not stored in it: the container supplies it.

## 1. Blocks

The decoded data is cut into **blocks** of 262,144 bytes (256 KiB); the last block is whatever is
left. Each block starts with two bytes.

| Byte | Bits | Meaning |
|------|------|---------|
| 0 | low 4 | Always `0xC`. |
| 0 | `0x40` | The block is stored: its decoded bytes follow as they are. |
| 0 | `0x80` | Set on the first block of every stream seen. The decoder ignores it. |
| 0 | `0x10`, `0x20` | Not seen. The decoder refuses them. |
| 1 | low 7 | Stream type: 10 for this format. 5, 6, 11 and 12 are other members of the family. |
| 1 | `0x80` | Not seen (the reference refuses it on compressed data). The decoder refuses it. |

A stored block may carry any of the five stream types, since nothing in it depends on the type.
That is how the scrubbed fixtures of the Palworld core are written: `0xCC 0x06`, then the bytes.

A block that is not stored holds one **group**.

## 2. Groups

A group starts with a 3-byte big-endian word.

- The low 18 bits are the group's size in bytes, less one.
- If the size field is `0x3FFFF` and the bit above it is set, the block is **one byte repeated**:
  a single byte follows, and it fills the whole block.
- The size field `0x3FFFF` without that bit does not occur. A block that does not compress at its
  full size is sent as a stored block instead.
- The remaining high bits are ignored.

Otherwise the group's bytes follow:

- If the group is exactly as long as its block, it is **stored**: the bytes are the data.
- If it is shorter, it holds the block's **chunks**, one after another, and nothing else. This is
  the only place the stream type matters: this document describes type 10.

## 3. Chunks

A block's data is cut into **chunks** of 131,072 bytes (128 KiB), so a group holds one or two.
A chunk starts with a 3-byte big-endian word.

**Top bit set: matches and literals.** The next 4 bits are the literal mode (1: literals are
stored as they are; 0: as differences, see section 6; other values not seen). The low 19 bits are
the size of the chunk's body, which follows.

- If the body is exactly as long as the chunk's data, the chunk is **stored** and the mode is 0.
- Otherwise the body is laid out as in section 6.

**Top bit clear: a single array.** The three bytes are the start of a 5-byte coded array header
(section 4), and the array is the chunk's data. Stored arrays and short headers do not occur here.

## 4. Arrays

Almost everything in a chunk is a **byte array** with a header saying how it is stored. The first
byte gives the form: bit 7 is the short-header flag, bits 4 to 6 the type.

| Type | Header | Meaning |
|------|--------|---------|
| 0 | 3 bytes, big-endian: 4 flag/type bits, 20-bit length | Stored: the bytes follow. |
| 0, short | 2 bytes, big-endian: 4 flag/type bits, 12-bit length | Stored. |
| 2, 3, 4 | 5 bytes, big-endian: 4 flag/type bits, 18 bits of decoded length less one, 18 bits of coded length | Coded. |
| 2, 3, 4, short | 3 bytes, big-endian: 4 flag/type bits, 10 bits of (decoded length − coded length − 1), 10 bits of coded length | Coded. |

Type 2 is Huffman with three streams, type 4 Huffman with six, type 3 run-length. Types 1 and 5
exist in the family but were not seen in type-10 saves; the decoder refuses them.

A coded array is always shorter than its data. One that would not be is sent stored.

### 4.1 Huffman arrays (types 2 and 4)

The payload is a description of the code, padded with zero bits to a whole byte, then the coded
bytes.

Codes are at most 11 bits long and canonical: sort the byte values in use by code length, then by
value, and hand out codes in increasing order, each length continuing from the last code of the
length before, shifted left. The code is complete (the lengths satisfy the Kraft equality), so
every 11-bit window starts with exactly one code.

#### The code description

Read most significant bit first.

1. Two bits, `10`. (Other values select encodings not seen in type-10 saves.)
2. Two bits: `k`, the number of low bits in the Rice codes below.
3. Eight bits: the number of byte values in use, less one. Call the number `n`.
4. If `n` is less than 256, the number of **spans**, `s`. It is a number below
   `c = 2 × min(n, 257 − n)`, written in the fewest bits that can tell `c` values apart: with
   `w` the number of bits in `c − 1`, the lowest `2^w − c` values take `w − 1` bits, and the
   rest take `w` bits and are stored with `2^w − c` added. If `n` is 256, there are no spans.
5. `n + s` unary numbers: each is a count of zero bits ended by a one bit. The first `n` belong to
   the code lengths, the last `s` to the spans.
6. `n` times `k` bits: the low bits of the code lengths' Rice codes.
7. The spans' extra bits (below).

**Code lengths.** For the i-th byte value in use (in increasing order of value), the Rice code
is `v = (unary << k) | low bits`. It is a signed step: even `v` is `+v/2`, odd `v` is
`−(v+1)/2`. A running estimate starts at 30; the length is `step + (estimate >> 2) + 1`, and
the step is then added to the estimate. Lengths are 1 to 11.

**Spans.** The byte values in use are given as alternating spans: a count of unused values to
skip, then a count of used values to take, and so on. If `s` is odd the list starts with a skip;
if even, with a take (the value 0 is in use). After the last span, the values still owed are taken
consecutively. With `u` the span's unary number:

- a skip is `(1 << (u+1) | next u+1 bits) − 1`;
- a take is `1 << u | next u bits`.

#### The coded bytes

Three streams of bits carry the codes. Output byte 0 comes from the first stream, byte 1 from the
second, byte 2 from the third, byte 3 from the first again, and so on. Every stream is read least
significant bit first, and a code's bits arrive most significant first.

A **group of three streams** is:

1. a 16-bit little-endian size;
2. the first stream, that many bytes, read forward;
3. the rest, shared by the other two: the third stream is read forward from its start, the second
   backward from its end (its first bit is the low bit of the last byte).

Type 2 is one such group for the whole array. Type 4 is a 24-bit little-endian size, a group of
that size for the first half of the array (the larger half when the length is odd), and a second
group for the rest.

### 4.2 Run-length arrays (type 3)

If the payload is a single byte, the array is that byte repeated for its whole length. Otherwise
the array is made of **literal** bytes and **command** bytes.

- If the payload's first byte is 0, the literals follow it and run forward; the commands sit at
  the end of the payload. The two meet exactly when the array is complete.
- Otherwise the payload starts with another array (section 4), which holds the literals, and the
  commands are all the bytes after it.

Commands are read **backward**, from the last byte of the payload. A command copies some literals
to the output and then writes the **fill byte** some number of times. The fill byte starts as 0.

| First byte | Following bytes | Literals copied | Fill bytes written |
|------------|-----------------|-----------------|--------------------|
| `0x30`–`0xFF` | none | 15 − (low 4 bits) | high 4 bits |
| `0x10`–`0x2F` | one, `b` | low 6 bits of `v` | `v >> 6`, where `v = (first << 8 \| b) − 0x1000` |
| `0x09`–`0x0F` | one, `b` | 0 | `(((first − 9) << 8 \| b) + 1) × 128` |
| `0x02`–`0x08` | one, `b` | `(((first − 2) << 8 \| b) + 1) × 64` | 0 |
| `0x01` | none | none: the next literal becomes the fill byte | 0 |
| `0x00` | none | 15 | 0 |

("Following" bytes are the ones read next, so they sit before the first byte in the payload.)

## 5. Offsets and how matches copy

A match copies bytes that were decoded earlier, from `offset` bytes back. The source may be in an
earlier chunk or block: an offset can reach all the way to the start of the stream. Offsets below
8 do not occur. When the offset is shorter than the match, the copy repeats what it has just
written.

Each chunk starts with the "last offset" set to 8.

## 6. A chunk of matches and literals

The body holds, in order:

1. **Only in the first chunk of a stream:** 8 bytes, copied to the output as they are.
2. The **literals**: an array.
3. The **commands**: an array.
4. **Only if the chunk is longer than 65,536 bytes:** a 16-bit little-endian number, the index of
   the first command of the chunk's second half.
5. The **near offsets**. A 16-bit little-endian count. If it is `0xFFFF`, two arrays of equal
   length follow, the high bytes and then the low bytes of the offsets. Otherwise that many 16-bit
   little-endian offsets follow.
6. The **far offset counts**: 3 bytes, little-endian; the high 12 bits count the first half's far
   offsets and the low 12 bits the second half's. A count of 4095 is replaced by a 16-bit
   little-endian number that follows (the first half's before the second's).
7. The **far offsets**, the first half's and then the second's. Each is 3 bytes, little-endian;
   if that value is `0xC00000` or more, a fourth byte follows and adds `byte << 22`.
8. The **long lengths**: everything that is left.

The chunk is decoded in two halves of 65,536 output bytes. Each half runs its own commands (up to
the split index, then from it) and its own far offsets; literals, near offsets and long lengths
are shared and simply continue. A command never writes across the end of its half.

A **long length** is one byte; if it is above 251, it is that byte plus 4 times a following
16-bit little-endian number.

| Command | Meaning |
|---------|---------|
| 24–255 | Copy `command & 7` literals. Then a match of `(command >> 3) & 15` bytes: at the last offset if bit 7 is set, else at the next near offset. |
| 0 | Copy a long run of literals: a long length plus 64. |
| 1 | A match of a long length plus 91 bytes at the next near offset. |
| 2 | A match of a long length plus 29 bytes at the next far offset. |
| 3–23 | A match of `command + 5` bytes at the next far offset. |

A **far offset** is stored less the number of bytes already written in the current half: the real
offset is the stored value plus the distance from the start of the half to the current position.

Every match sets the last offset. When a half's commands are done, the rest of the half is filled
from the literals.

In literal mode 1 a literal is the output byte. In mode 0 it is the output byte minus the byte
`last offset` back (modulo 256), so the decoder adds that byte back.

## What is not covered

The decoder returns `Error::Unsupported` for these, since none appeared in a save and none could
be confirmed by experiment:

- The other stream types of the family (5, 6, 11, 12) when their blocks are not stored.
- Array types 1 and 5.
- Huffman code descriptions that do not start with the bits `10`.
- Chunk literal modes other than 0 and 1.
- Block header bits `0x10` and `0x20`, and the high bit of the type byte (which, by its position
  and the reference's behaviour, probably announces per-block checksums).
- Run-length arrays nested more than four deep.

Two fields are decoded as described but were only exercised by this repository's own test
encoder, checked against the reference, and never by a real save: the 16-bit replacement for a far
offset count of 4095, and the short header on stored arrays.
