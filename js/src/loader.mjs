// The whole JavaScript side of kraken-decode: instantiate the WebAssembly
// module, copy bytes in, call it, copy bytes out. Written by hand; there is no
// bindings generator and nothing to install.

const KINDS = ["", "truncated", "corrupt", "unsupported", "size mismatch", "failed"];

/** What `decompress` throws. `kind` is one of "truncated", "corrupt", "unsupported", "size mismatch". */
export class KrakenDecodeError extends Error {
  constructor(kind, detail) {
    super(detail ? `kraken-decode: ${kind}: ${detail}` : `kraken-decode: ${kind}`);
    this.name = "KrakenDecodeError";
    this.kind = kind;
    this.detail = detail;
  }
}

/**
 * Instantiate the decoder from the bytes of `kraken_decode_wasm.wasm`.
 * Synchronous: the module is a few tens of kilobytes.
 */
export function instantiate(wasmBytes) {
  const { exports: wasm } = new WebAssembly.Instance(new WebAssembly.Module(wasmBytes), {});
  // The memory can grow (and its buffer be replaced) on any allocation, so take a fresh view each time.
  const bytes = () => new Uint8Array(wasm.memory.buffer);

  /**
   * Decode `data` (a whole compressed stream) to exactly `rawSize` bytes.
   * Returns a new Uint8Array that the caller owns.
   */
  function decompress(data, rawSize) {
    if (!(data instanceof Uint8Array)) throw new TypeError("kraken-decode: data must be a Uint8Array");
    if (!Number.isInteger(rawSize) || rawSize < 0 || rawSize > 0x7fffffff) throw new RangeError("kraken-decode: rawSize must be a non-negative integer");
    // Copy the input's own bytes once: a subclass could report a different `length` each time it is asked.
    const input = new Uint8Array(data);
    const size = input.length;
    let src = -1, dst = -1;
    try {
      // Addresses are unsigned; WebAssembly hands them to JavaScript as signed 32-bit numbers.
      src = wasm.kd_alloc(size) >>> 0;
      dst = wasm.kd_alloc(rawSize) >>> 0;
      bytes().set(input, src);
      const code = wasm.kd_decode(src, size, dst, rawSize);
      if (code !== 0) {
        const at = wasm.kd_detail_ptr() >>> 0;
        const detail = new TextDecoder().decode(bytes().subarray(at, at + wasm.kd_detail_len()));
        throw new KrakenDecodeError(KINDS[code] ?? "failed", detail);
      }
      return bytes().slice(dst, dst + rawSize);
    } finally {
      if (src >= 0) wasm.kd_free(src, size);
      if (dst >= 0) wasm.kd_free(dst, rawSize);
    }
  }

  return { decompress };
}
