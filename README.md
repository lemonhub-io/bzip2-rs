# bzip2-rs

A faithful, line-for-line Rust port of **bzip2/libbzip2 1.0.8**
(upstream commit `6a8690fc`, the 13-Jul-2019 release by Julian R
Seward).

This is a complete rewrite in Rust — not a wrapper, not a binding.
Every algorithm, the streaming state machines, the CLI, the recovery
tool, and the full public C API/ABI are reimplemented in Rust and
verified byte-for-byte against the original C implementation.

## What is included

| Upstream C source | Rust |
|---|---|
| `bzlib.c`, `bzlib.h`, `bzlib_private.h` | `src/bzlib.rs`, `src/common.rs`, `src/state.rs` |
| `compress.c` | `src/compress.rs` |
| `decompress.c` | `src/decompress.rs` |
| `blocksort.c` | `src/blocksort.rs` |
| `huffman.c` | `src/huffman.rs` |
| `crctable.c`, `randtable.c` | `src/tables.rs` |
| libc usage | `src/sys.rs` |
| `bzip2.c` | `src/bin/bzip2.rs` |
| `bzip2recover.c` | `src/bin/bzip2recover.rs` |
| `dlltest.c` | `src/bin/minibz2.rs` |
| `spewG.c`, `unzcrash.c`, `mk251.c` | `src/bin/{spewg,unzcrash,mk251}.rs` |
| `bzdiff`, `bzgrep`, `bzmore` (+ man pages) | `bin-share/` (unchanged POSIX shell) |
| docs, manual, samples, `words*` | repo root and `docs/` |

See `PORTING.md` for the detailed source map and translation choices.

## Highlights

- `#[repr(C)]` `bz_stream`/`BZFILE` and `#[no_mangle] extern "C"`
  `BZ2_*` exports: the build produces real `libbz2.a` / `libbz2.so`
  drop-in libraries (25 exported symbols). A C program compiled
  against the upstream `bzlib.h` links and runs unchanged.
- Custom `bzalloc`/`bzfree` allocator hooks are honored.
- Fast and small decompression modes both implemented.
- The decompressor reproduces the C suspension/resumption semantics
  (`BZ_OUTBUFF_FULL`, `BZ_UNEXPECTED_EOF`) exactly.
- Compression is byte-identical to the C implementation.

## Build

```sh
cargo build --release
```

Produces the library (`rlib`, `staticlib`, `cdylib`) and six
binaries: `bzip2`, `bzip2recover`, `minibz2`, `mk251`, `spewG`,
`unzcrash`.

## Test

```sh
cargo test          # smoke + byte-exact-diff + chunked-stream tests
```

`tests/diff.rs` compares output against the original C binary
(default `../bzip2/bzip2`, override with `BZIP2_C=/path/to/bzip2`).

## Verification performed

- Byte-identical compressed output vs the C oracle across the test
  corpus and block sizes `-1`..`-9`, including `--exponential` and a
  300-case randomized differential fuzz run.
- Cross-decompression both directions, fast and small modes.
- Upstream `make test` equivalent on `sample{1,2,3}` — pass.
- 58-case CLI parity harness (flags, exit codes, stdout/stderr,
  file trees, metadata, progname dispatch, signal cleanup, `BZIP2`
  env var) — all identical to the C binary.
- `bzip2recover` logs and recovered blocks identical; `mk251` and
  `spewG` byte-identical; `unzcrash` full 11,112-bit-flip sweep with
  no crashes.

## License

The Rust port is licensed under **Apache-2.0** (see `LICENSE`).

It is a translation of bzip2/libbzip2 1.0.8, copyright (C) 1996-2019
Julian R Seward, whose original license is preserved in
`LICENSE-upstream` and whose copyright notices are retained in the
source file headers. See `NOTICE`.
