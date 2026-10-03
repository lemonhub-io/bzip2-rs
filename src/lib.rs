/*-------------------------------------------------------------*/
/*--- bzip2 in Rust: a literal port of bzip2/libbzip2 1.0.8  ---*/
/*---                                                lib.rs ---*/
/*-------------------------------------------------------------*/

/* This file is part of the Rust port of bzip2/libbzip2.

   The crate reproduces the complete libbzip2 1.0.8 implementation:
   the compression/decompression algorithms (blocksort.c, compress.c,
   decompress.c, huffman.c), the CRC and randomisation tables
   (crctable.c, randtable.c), and the full public API (bzlib.c)
   with a C-compatible ABI. */

#![allow(non_snake_case)]
#![allow(non_camel_case_types)]
#![allow(clippy::missing_safety_doc)]
/* This crate is a deliberately literal port of C.  Several clippy
   style lints fight the translation: `loop{}` blocks used as
   break-targets when emulating C `goto`/case-label fallthrough,
   index loops over shared buffers that alias each other (and hence
   cannot be iterators), `b"...\0"` C-format string literals, and
   explicit `as` casts where the C macro operands were untyped. */
#![allow(clippy::never_loop)]
#![allow(clippy::needless_range_loop)]
#![allow(clippy::unnecessary_cast)]
#![allow(clippy::manual_c_str_literals)]
#![allow(clippy::manual_clamp)]
#![allow(clippy::no_effect)]
#![allow(clippy::manual_swap)]
#![allow(clippy::bool_comparison)]
#![allow(clippy::if_same_then_else)]
#![allow(clippy::needless_late_init)]

pub mod blocksort;
pub mod bzlib;
pub mod common;
pub mod compress;
pub mod decompress;
pub mod huffman;
pub mod state;
pub mod sys;
pub mod tables;

pub use bzlib::*;
pub use common::*;
