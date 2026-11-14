// Parity check against a reference decoder, used strictly as a black box.
//
// Decodes every Palworld `.sav` (and every `.kdv` test vector) under a folder with this crate's WebAssembly
// build and with a reference module, and compares the bytes. Neither the
// saves nor the reference are part of this repository:
//
//   KRAKEN_PARITY_DIR     a folder of real saves, kept outside the repository
//   KRAKEN_PARITY_ORACLE  path to a module exporting decompress(data, rawSize)
//
//   node js/build.mjs && node tools/parity.mjs
//
// See CONTRIBUTING.md ("The clean-room rule") before pointing this at anything.

import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { decompress } from "../js/dist/kraken-decode.mjs";

const dir = process.env.KRAKEN_PARITY_DIR;
const oraclePath = process.env.KRAKEN_PARITY_ORACLE;
if (!dir || !oraclePath) {
  console.error("set KRAKEN_PARITY_DIR and KRAKEN_PARITY_ORACLE (see the comment at the top of this file)");
  process.exit(2);
}
const oracle = await import(pathToFileURL(resolve(oraclePath)).href);

const files = [];
(function walk(d) {
  for (const name of readdirSync(d)) {
    const p = join(d, name);
    if (statSync(p).isDirectory()) walk(p);
    else if ((name.endsWith(".sav") && statSync(p).size > 12) || name.endsWith(".kdv")) files.push(p);
  }
})(dir);
files.sort();

let same = 0, different = 0, skipped = 0, bytes = 0, ours = 0, theirs = 0;
for (const file of files) {
  const data = readFileSync(file);
  const rawSize = data.readUInt32LE(0);
  let body;
  if (file.endsWith(".kdv")) {
    // A test vector from `export_vectors`: the decoded length, then the stream.
    body = new Uint8Array(data.buffer, data.byteOffset + 4, data.length - 4);
  } else {
    if (data.subarray(8, 11).toString("latin1") !== "PlM") { skipped++; continue; }
    body = new Uint8Array(data.buffer, data.byteOffset + 12, data.readUInt32LE(4));
  }
  let t = performance.now();
  let want;
  try { want = Uint8Array.from(oracle.decompress(body, rawSize)); } catch (e) { different++; console.log(`REFERENCE REFUSED ${file}: ${e?.message ?? e}`); continue; }
  theirs += performance.now() - t;
  let got;
  t = performance.now();
  try { got = decompress(body, rawSize); } catch (e) { got = e; }
  ours += performance.now() - t;
  if (got instanceof Uint8Array && Buffer.compare(got, want) === 0) { same++; bytes += rawSize; }
  else { different++; console.log(`DIFFERENT ${file}: ${got instanceof Error ? got.message : "bytes differ"}`); }
}
console.log(`${same} identical, ${different} different, ${skipped} skipped (not PlM), ${(bytes / 1e6).toFixed(1)} MB compared`);
console.log(`time: kraken-decode ${ours.toFixed(0)} ms, reference ${theirs.toFixed(0)} ms`);
process.exit(different ? 1 : 0);
