// The JavaScript loader against the committed fixtures (tests/fixtures).
// Build first: cargo build -p kraken-decode-wasm --target wasm32-unknown-unknown --release && node js/build.mjs

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { KrakenDecodeError, decompress } from "./dist/kraken-decode.mjs";

const fixture = (name) => readFileSync(new URL(`../tests/fixtures/${name}`, import.meta.url));

// The scrubbed player save: a PlM container around stored blocks.
const sav = fixture("player.sav");
const gvas = decompress(sav.subarray(12, 12 + sav.readUInt32LE(4)), sav.readUInt32LE(0));

test("decodes the scrubbed save's stored blocks", () => {
  assert.equal(gvas.length, 27462);
  assert.equal(Buffer.from(gvas.subarray(0, 4)).toString("latin1"), "GVAS");
});

for (const name of ["plain", "huffman", "runs", "array"]) {
  test(`decodes the ${name} stream to the same bytes`, () => {
    const file = fixture(`player.${name}.kdv`);
    const out = decompress(file.subarray(4), file.readUInt32LE(0));
    assert.ok(out instanceof Uint8Array);
    assert.equal(Buffer.compare(out, gvas), 0);
  });
}

test("returns a copy the caller owns", () => {
  const file = fixture("player.plain.kdv");
  const first = decompress(file.subarray(4), file.readUInt32LE(0));
  const kept = Uint8Array.from(first);
  decompress(fixture("player.huffman.kdv").subarray(4), 27462);
  assert.deepEqual(first, kept);
});

test("throws a typed error on a damaged or truncated stream", () => {
  const file = fixture("player.huffman.kdv");
  const stream = Uint8Array.from(file.subarray(4));
  assert.throws(() => decompress(stream.subarray(0, 1000), 27462), (e) => e instanceof KrakenDecodeError && e.kind === "truncated");
  assert.throws(() => decompress(stream, 27461), KrakenDecodeError);
  stream.fill(0xff, 0, 2);
  assert.throws(() => decompress(stream, 27462), (e) => e instanceof KrakenDecodeError && e.kind === "corrupt" && e.detail === "block header");
  assert.throws(() => decompress("nope", 4), TypeError);
  assert.throws(() => decompress(stream, -1), RangeError);
});

test("survives many calls without leaking the instance's memory", () => {
  const file = fixture("player.runs.kdv");
  for (let i = 0; i < 2000; i++) decompress(file.subarray(4), 27462);
});
