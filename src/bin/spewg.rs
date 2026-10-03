/*-----------------------------------------------------------*/
/*--- spewG.c -- spew a gigantic compressible file          ---*/
/*---                       ported to Rust, same behavior   ---*/
/*-----------------------------------------------------------*/

/* ------------------------------------------------------------------
   This file is part of bzip2/libbzip2, a program and library for
   lossless, block-sorting data compression.

   bzip2/libbzip2 version 1.0.8 of 13 July 2019
   Copyright (C) 1996-2019 Julian Seward <jseward@acm.org>
   ------------------------------------------------------------------ */

/* spew out a thoroughly gigantic file designed so that bzip2
   can compress it reasonably rapidly.  This is to help test
   support for large files (> 2GB) in a reasonable amount of time. */

#![allow(static_mut_refs)]
#![allow(clippy::manual_c_str_literals)]
#![allow(clippy::unnecessary_cast)]

use bz2::sys;
use std::os::raw::{c_char, c_int};

const MEGABYTES: i64 = 5000;
const N_BUF: usize = 1000000;

extern "C" {
    fn setbuffer(stream: *mut sys::FILE, buf: *mut c_char, size: c_int);
}

static mut BUF: [c_char; N_BUF] = [0; N_BUF];

fn main() {
    unsafe {
        sys::srandom(1);
        setbuffer(sys::stdout, BUF.as_mut_ptr(), N_BUF as c_int);
        let mut kk: i64 = 0;
        while kk < MEGABYTES * 515 {
            let p = 25 + (sys::random() % 50) as i32;
            for _ in 0..p {
                sys::fputs(
                    b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\0".as_ptr() as *const c_char,
                    sys::stdout,
                );
            }
            for _ in 0..p - 1 {
                sys::fputs(
                    b"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\0".as_ptr() as *const c_char,
                    sys::stdout,
                );
            }
            for _ in 0..p + 1 {
                sys::fputs(
                    b"ccccccccccccccccccccccccccccccccccccc\0".as_ptr() as *const c_char,
                    sys::stdout,
                );
            }
            kk += 3;
        }
        sys::fflush(sys::stdout);
    }
}
