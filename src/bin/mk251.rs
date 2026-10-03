/*-----------------------------------------------------------*/
/*--- mk251.c -- spew a long sequence of byte 251           ---*/
/*---                       ported to Rust, same behavior   ---*/
/*-----------------------------------------------------------*/

/* ------------------------------------------------------------------
   This file is part of bzip2/libbzip2, a program and library for
   lossless, block-sorting data compression.

   bzip2/libbzip2 version 1.0.8 of 13 July 2019
   Copyright (C) 1996-2019 Julian Seward <jseward@acm.org>
   ------------------------------------------------------------------ */

/* Spew out a long sequence of the byte 251.  When fed to bzip2
   versions 1.0.0 or 1.0.1, causes it to die with internal error
   1007 in blocksort.c.  This assertion misses an extremely rare
   case, which is fixed in this version (1.0.2) and above. */

use bz2::sys;
use std::os::raw::c_int;

fn main() {
    unsafe {
        for _ in 0..48500000i32 {
            sys::putchar(251 as c_int);
        }
    }
}
