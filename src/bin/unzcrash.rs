/*-----------------------------------------------------------*/
/*--- unzcrash.c -- test robustness vs corrupted data       ---*/
/*---                       ported to Rust, same behavior   ---*/
/*-----------------------------------------------------------*/

/* ------------------------------------------------------------------
   This file is part of bzip2/libbzip2, a program and library for
   lossless, block-sorting data compression.

   bzip2/libbzip2 version 1.0.8 of 13 July 2019
   Copyright (C) 1996-2019 Julian Seward <jseward@acm.org>
   ------------------------------------------------------------------ */

/* A test program written to test robustness to decompression of
   corrupted data.  Usage is
       unzcrash filename
   and the program will read the specified file, compress it (in memory),
   and then repeatedly decompress it, each time with a different bit of
   the compressed data inverted, so as to test all possible one-bit errors.
   This should not cause any invalid memory accesses. */

#![allow(static_mut_refs)]
#![allow(clippy::manual_c_str_literals)]
#![allow(clippy::unnecessary_cast)]
#![allow(clippy::needless_range_loop)]

use bz2::sys;
use bz2::*;
use std::ffi::CString;
use std::os::raw::{c_char, c_uint, c_void};
use std::os::unix::ffi::OsStrExt;

const M_BLOCK: usize = 1000000;
const M_BLOCK_OUT: usize = M_BLOCK + 1000000;
const ZBUF_LEN: usize = M_BLOCK + 600 + (M_BLOCK / 100);

static mut INBUF: [u8; M_BLOCK] = [0; M_BLOCK];
static mut OUTBUF: [u8; M_BLOCK_OUT] = [0; M_BLOCK_OUT];
static mut ZBUF: [u8; ZBUF_LEN] = [0; ZBUF_LEN];

const BZERRORSTRINGS: [&[u8]; 15] = [
    b"OK\0",
    b"SEQUENCE_ERROR\0",
    b"PARAM_ERROR\0",
    b"MEM_ERROR\0",
    b"DATA_ERROR\0",
    b"DATA_ERROR_MAGIC\0",
    b"IO_ERROR\0",
    b"UNEXPECTED_EOF\0",
    b"OUTBUFF_FULL\0",
    b"???\0",
    b"???\0",
    b"???\0",
    b"???\0",
    b"???\0",
    b"???\0",
];

unsafe fn flip_bit(bit: usize) {
    let byteno = bit / 8;
    let bitno = bit % 8;
    let mask: u8 = 1 << bitno;
    ZBUF[byteno] ^= mask;
}

fn main() {
    unsafe {
        let argv: Vec<Vec<u8>> = std::env::args_os()
            .map(|a| a.as_bytes().to_vec())
            .collect();
        if argv.len() != 2 {
            sys::fprintf(
                sys::stderr,
                b"usage: unzcrash filename\n\0".as_ptr() as *const c_char,
            );
            std::process::exit(1);
        }

        let fname = CString::new(argv[1].clone()).unwrap();
        let f = sys::fopen(fname.as_ptr(), b"r\0".as_ptr() as *const c_char);
        if f.is_null() {
            sys::fprintf(
                sys::stderr,
                b"unzcrash: can't open %s\n\0".as_ptr() as *const c_char,
                fname.as_ptr(),
            );
            std::process::exit(1);
        }

        let n_in = sys::fread(INBUF.as_mut_ptr() as *mut c_void, 1, M_BLOCK, f) as usize;
        sys::fprintf(
            sys::stderr,
            b"%d bytes read\n\0".as_ptr() as *const c_char,
            n_in as i32,
        );

        let mut n_z: c_uint = M_BLOCK as c_uint;
        let r = BZ2_bzBuffToBuffCompress(
            ZBUF.as_mut_ptr() as *mut c_char,
            &mut n_z,
            INBUF.as_mut_ptr() as *mut c_char,
            n_in as c_uint,
            9,
            0,
            30,
        );
        assert!(r == BZ_OK);
        sys::fprintf(
            sys::stderr,
            b"%d after compression\n\0".as_ptr() as *const c_char,
            n_z as i32,
        );

        let nz = n_z as usize;
        for bit in 0..nz * 8 {
            sys::fprintf(
                sys::stderr,
                b"bit %d  \0".as_ptr() as *const c_char,
                bit as i32,
            );
            flip_bit(bit);
            let mut n_out: c_uint = M_BLOCK_OUT as c_uint;
            let r = BZ2_bzBuffToBuffDecompress(
                OUTBUF.as_mut_ptr() as *mut c_char,
                &mut n_out,
                ZBUF.as_mut_ptr() as *mut c_char,
                n_z,
                0,
                0,
            );
            let estr = if (-r) >= 0 && (-r) < 15 {
                BZERRORSTRINGS[(-r) as usize]
            } else {
                b"???\0"
            };
            sys::fprintf(
                sys::stderr,
                b" %d  %s \0".as_ptr() as *const c_char,
                r,
                estr.as_ptr() as *const c_char,
            );

            if r != BZ_OK {
                sys::fprintf(sys::stderr, b"\n\0".as_ptr() as *const c_char);
            } else {
                if n_out as usize != n_in {
                    sys::fprintf(
                        sys::stderr,
                        b"nIn/nOut mismatch %d %d\n\0".as_ptr() as *const c_char,
                        n_in as i32,
                        n_out as i32,
                    );
                    std::process::exit(1);
                } else {
                    let mut i = 0usize;
                    while i < n_out as usize {
                        if INBUF[i] != OUTBUF[i] {
                            sys::fprintf(
                                sys::stderr,
                                b"mismatch at %d\n\0".as_ptr() as *const c_char,
                                i as i32,
                            );
                            std::process::exit(1);
                        }
                        i += 1;
                    }
                    if i == n_out as usize {
                        sys::fprintf(
                            sys::stderr,
                            b"really ok!\n\0".as_ptr() as *const c_char,
                        );
                    }
                }
            }

            flip_bit(bit);
        }

        sys::fprintf(sys::stderr, b"all ok\n\0".as_ptr() as *const c_char);
    }
}
