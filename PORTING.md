# bzip2-rs: a faithful Rust port of bzip2/libbzip2 1.0.8

This is a line-faithful Rust translation of the original C source
(bzip2 1.0.8, commit `6a8690fc`, the 13-Jul-2019 release), not a
wrapper or binding. Every algorithm, the streaming state machines,
and the public C ABI are reimplemented in Rust.

## Source map

| Upstream C file      | Rust file                     |
|----------------------|-------------------------------|
| `bzlib.h`            | `src/common.rs`, `src/bzlib.rs` |
| `bzlib_private.h`    | `src/state.rs`, `src/common.rs` |
| `crctable.c` + `randtable.c` | `src/tables.rs`      |
| `huffman.c`          | `src/huffman.rs`              |
| `blocksort.c`        | `src/blocksort.rs`            |
| `compress.c`         | `src/compress.rs`             |
| `decompress.c`       | `src/decompress.rs`           |
| `bzlib.c`            | `src/bzlib.rs`                |
| libc usage           | `src/sys.rs`                  |
| `bzip2.c`            | `src/bin/bzip2.rs`            |
| `bzip2recover.c`     | `src/bin/bzip2recover.rs`     |
| `dlltest.c`          | `src/bin/minibz2.rs`          |
| `spewG.c`            | `src/bin/spewg.rs`            |
| `unzcrash.c`         | `src/bin/unzcrash.rs`         |
| `mk251.c`            | `src/bin/mk251.rs`            |
| `bzdiff`/`bzgrep`/`bzmore` + man pages | `bin-share/` (unchanged POSIX shell) |
| docs / manual / samples / words* / LICENSE / README / CHANGES | repo root and `docs/` |

## Notable translation choices

- `#[repr(C)]` `bz_stream`/`BZFILE` and `#[no_mangle] extern "C"` exports
  (`BZ2_*`) — the crate builds real `libbz2.a`/`libbz2.so` drop-ins.
- Custom `bzalloc`/`bzfree` allocator hooks honored via `BzAlloc`.
- C `arr2` aliasing (`u32` eclass / `u8` block / `u16` quadrant) is
  reproduced with little-endian accessors over a shared buffer.
- The decompressor's resumable `case`-label suspension is emulated
  with a dispatch loop over `s.state`; suspension points and
  `BZ_OUTBUFF_FULL`/`BZ_UNEXPECTED_EOF` semantics are preserved.
- `goto` sites are emulated with labeled loops/`continue`.

## Build

```sh
cargo build --release          # library (rlib/staticlib/cdylib) + all 6 binaries
cargo test                     # smoke + byte-exact-diff + chunked-stream tests
```

## Verification performed

- `tests/smoke.rs` — round-trips incl. empty, repetitive, random,
  multiblock; fast and small decompression modes.
- `tests/diff.rs` — byte-identical compressed output vs the C oracle
  across the corpus and block sizes 1–9; cross-decompression both ways.
- `tests/stream.rs` — chunked streaming with tiny buffers.
- Upstream `make test` equivalent on `sample{1,2,3}` — pass.
- CLI parity harness: 58 cases of flags/exit codes/stdout/stderr/file
  trees identical to the C binary; metadata (mode, mtime), progname
  dispatch (`bunzip2`/`bzcat`), signal cleanup, `BZIP2` env var.
- `bzip2recover` — recovery logs + recovered blocks identical.
- `mk251`, `spewG` — byte-identical output.
- `unzcrash` — full 11,112 single-bit-flip sweep, no crashes.
- 300-case randomized differential fuzz + 15MB `--exponential` run —
  all byte-identical.
