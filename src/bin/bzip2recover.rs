/*-----------------------------------------------------------*/
/*--- Block recoverer program for bzip2                   ---*/
/*---                                      bzip2recover.c ---*/
/*---                       ported to Rust, same behavior   ---*/
/*-----------------------------------------------------------*/

/* ------------------------------------------------------------------
   This file is part of bzip2/libbzip2, a program and library for
   lossless, block-sorting data compression.

   bzip2/libbzip2 version 1.0.8 of 13 July 2019
   Copyright (C) 1996-2019 Julian Seward <jseward@acm.org>

   Please read the WARNING, DISCLAIMER and PATENTS sections in the
   README file.

   This program is released under the terms of the license contained
   in the file LICENSE.
   ------------------------------------------------------------------ */

/* This program is a complete hack and should be rewritten properly.
   It isn't very complicated. */

/* Literal translation of bzip2recover.c.  MaybeUInt64 is u64. */

#![allow(non_snake_case)]
#![allow(static_mut_refs)]
/* literal C port: C-format `b"...\0"` literals are intentional */
#![allow(clippy::manual_c_str_literals)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::unnecessary_cast)]
#![allow(clippy::needless_range_loop)]

use bz2::sys;
use std::os::raw::{c_char, c_int, c_void};
use std::os::unix::ffi::OsStrExt;

const BZ_MAX_FILENAME: usize = 2000;
const BZ_MAX_HANDLED_BLOCKS: usize = 50000;

const BLOCK_HEADER_HI: u32 = 0x00003141;
const BLOCK_HEADER_LO: u32 = 0x59265359;
const BLOCK_ENDMARK_HI: u32 = 0x00001772;
const BLOCK_ENDMARK_LO: u32 = 0x45385090;

const BZ_HDR_B: u8 = 0x42; /* 'B' */
const BZ_HDR_Z: u8 = 0x5a; /* 'Z' */
const BZ_HDR_H: u8 = 0x68; /* 'h' */
const BZ_HDR_0: u8 = 0x30; /* '0' */

static mut IN_FILE_NAME: [c_char; BZ_MAX_FILENAME] = [0; BZ_MAX_FILENAME];
static mut OUT_FILE_NAME: [c_char; BZ_MAX_FILENAME] = [0; BZ_MAX_FILENAME];
static mut PROG_NAME: [c_char; BZ_MAX_FILENAME] = [0; BZ_MAX_FILENAME];

static mut BYTES_OUT: u64 = 0;
#[allow(dead_code)]
static mut BYTES_IN: u64 = 0;

fn in_file_name() -> *const c_char {
    unsafe { IN_FILE_NAME.as_ptr() }
}
fn prog_name() -> *const c_char {
    unsafe { PROG_NAME.as_ptr() }
}

/*---------------------------------------------------*/
/*--- I/O errors                                  ---*/
/*---------------------------------------------------*/

fn readError() -> ! {
    unsafe {
        sys::fprintf(
            sys::stderr,
            b"%s: I/O error reading `%s', possible reason follows.\n\0".as_ptr() as *const c_char,
            prog_name(),
            in_file_name(),
        );
        sys::perror(prog_name());
        sys::fprintf(
            sys::stderr,
            b"%s: warning: output file(s) may be incomplete.\n\0".as_ptr() as *const c_char,
            prog_name(),
        );
        sys::exit(1);
    }
}

fn writeError() -> ! {
    unsafe {
        sys::fprintf(
            sys::stderr,
            b"%s: I/O error reading `%s', possible reason follows.\n\0".as_ptr() as *const c_char,
            prog_name(),
            in_file_name(),
        );
        sys::perror(prog_name());
        sys::fprintf(
            sys::stderr,
            b"%s: warning: output file(s) may be incomplete.\n\0".as_ptr() as *const c_char,
            prog_name(),
        );
        sys::exit(1);
    }
}

fn mallocFail(n: i32) -> ! {
    unsafe {
        sys::fprintf(
            sys::stderr,
            b"%s: malloc failed on request for %d bytes.\n\0".as_ptr() as *const c_char,
            prog_name(),
            n,
        );
        sys::fprintf(
            sys::stderr,
            b"%s: warning: output file(s) may be incomplete.\n\0".as_ptr() as *const c_char,
            prog_name(),
        );
        sys::exit(1);
    }
}

fn tooManyBlocks(max_handled_blocks: i32) -> ! {
    unsafe {
        sys::fprintf(
            sys::stderr,
            b"%s: `%s' appears to contain more than %d blocks\n\0".as_ptr() as *const c_char,
            prog_name(),
            in_file_name(),
            max_handled_blocks,
        );
        sys::fprintf(
            sys::stderr,
            b"%s: and cannot be handled.  To fix, increase\n\0".as_ptr() as *const c_char,
            prog_name(),
        );
        sys::fprintf(
            sys::stderr,
            b"%s: BZ_MAX_HANDLED_BLOCKS in bzip2recover.c, and recompile.\n\0".as_ptr()
                as *const c_char,
            prog_name(),
        );
        sys::exit(1);
    }
}

/*---------------------------------------------------*/
/*--- Bit stream I/O                              ---*/
/*---------------------------------------------------*/

struct BitStream {
    handle: *mut sys::FILE,
    buffer: i32,
    buff_live: i32,
    mode: u8,
}

unsafe fn bsOpenReadStream(stream: *mut sys::FILE) -> *mut BitStream {
    let bs = sys::malloc(std::mem::size_of::<BitStream>()) as *mut BitStream;
    if bs.is_null() {
        mallocFail(std::mem::size_of::<BitStream>() as i32);
    }
    (*bs).handle = stream;
    (*bs).buffer = 0;
    (*bs).buff_live = 0;
    (*bs).mode = b'r';
    bs
}

unsafe fn bsOpenWriteStream(stream: *mut sys::FILE) -> *mut BitStream {
    let bs = sys::malloc(std::mem::size_of::<BitStream>()) as *mut BitStream;
    if bs.is_null() {
        mallocFail(std::mem::size_of::<BitStream>() as i32);
    }
    (*bs).handle = stream;
    (*bs).buffer = 0;
    (*bs).buff_live = 0;
    (*bs).mode = b'w';
    bs
}

unsafe fn bsPutBit(bs: *mut BitStream, bit: i32) {
    if (*bs).buff_live == 8 {
        let ret_val = sys::putc(((*bs).buffer & 0xff) as u8 as c_int, (*bs).handle);
        if ret_val == sys::EOF {
            writeError();
        }
        BYTES_OUT += 1;
        (*bs).buff_live = 1;
        (*bs).buffer = bit & 0x1;
    } else {
        (*bs).buffer = (((*bs).buffer) << 1) | (bit & 0x1);
        (*bs).buff_live += 1;
    }
}

/* Returns 0 or 1, or 2 to indicate EOF. */
unsafe fn bsGetBit(bs: *mut BitStream) -> i32 {
    if (*bs).buff_live > 0 {
        (*bs).buff_live -= 1;
        (((*bs).buffer) >> ((*bs).buff_live)) & 0x1
    } else {
        let ret_val = sys::getc((*bs).handle);
        if ret_val == sys::EOF {
            if sys::errno_val() != 0 {
                readError();
            }
            return 2;
        }
        (*bs).buff_live = 7;
        (*bs).buffer = ret_val;
        (((*bs).buffer) >> 7) & 0x1
    }
}

unsafe fn bsClose(bs: *mut BitStream) {
    if (*bs).mode == b'w' {
        while (*bs).buff_live < 8 {
            (*bs).buff_live += 1;
            (*bs).buffer <<= 1;
        }
        let ret_val = sys::putc(((*bs).buffer & 0xff) as u8 as c_int, (*bs).handle);
        if ret_val == sys::EOF {
            writeError();
        }
        BYTES_OUT += 1;
        if sys::fflush((*bs).handle) == sys::EOF {
            writeError();
        }
    }
    if sys::fclose((*bs).handle) == sys::EOF {
        if (*bs).mode == b'w' {
            writeError();
        } else {
            readError();
        }
    }
    sys::free(bs as *mut c_void);
}

unsafe fn bsPutUChar(bs: *mut BitStream, c: u8) {
    for i in (0..8).rev() {
        bsPutBit(bs, ((c as u32 >> i) & 0x1) as i32);
    }
}

unsafe fn bsPutUInt32(bs: *mut BitStream, c: u32) {
    for i in (0..32).rev() {
        bsPutBit(bs, ((c >> i) & 0x1) as i32);
    }
}

fn endsInBz2(name: &[u8]) -> bool {
    let n = name.len();
    if n <= 4 {
        return false;
    }
    name[n - 4] == b'.' && name[n - 3] == b'b' && name[n - 2] == b'z' && name[n - 1] == b'2'
}

static mut B_START: [u64; BZ_MAX_HANDLED_BLOCKS + 1] = [0; BZ_MAX_HANDLED_BLOCKS + 1];
static mut B_END: [u64; BZ_MAX_HANDLED_BLOCKS + 1] = [0; BZ_MAX_HANDLED_BLOCKS + 1];
static mut RB_START: [u64; BZ_MAX_HANDLED_BLOCKS] = [0; BZ_MAX_HANDLED_BLOCKS];
static mut RB_END: [u64; BZ_MAX_HANDLED_BLOCKS] = [0; BZ_MAX_HANDLED_BLOCKS];

#[allow(unused_assignments)]
fn main() {
    unsafe {
        let argv: Vec<Vec<u8>> = std::env::args_os()
            .map(|a| a.as_bytes().to_vec())
            .collect();
        let argc = argv.len() as c_int;

        /* strncpy(progName, argv[0], BZ_MAX_FILENAME-1); + NUL */
        {
            let src = &argv[0];
            let n = src.len().min(BZ_MAX_FILENAME - 1);
            std::ptr::copy_nonoverlapping(src.as_ptr(), PROG_NAME.as_mut_ptr() as *mut u8, n);
            PROG_NAME[n] = 0;
        }
        IN_FILE_NAME[0] = 0;
        OUT_FILE_NAME[0] = 0;

        sys::fprintf(
            sys::stderr,
            b"bzip2recover 1.0.8: extracts blocks from damaged .bz2 files.\n\0".as_ptr()
                as *const c_char,
        );

        if argc != 2 {
            sys::fprintf(
                sys::stderr,
                b"%s: usage is `%s damaged_file_name'.\n\0".as_ptr() as *const c_char,
                prog_name(),
                prog_name(),
            );
            /* sizeof(MaybeUInt64) == 8 here */
            sys::fprintf(
                sys::stderr,
                b"\trestrictions on size of recovered file: None\n\0".as_ptr() as *const c_char,
            );
            sys::exit(1);
        }

        if argv[1].len() >= BZ_MAX_FILENAME - 20 {
            sys::fprintf(
                sys::stderr,
                b"%s: supplied filename is suspiciously (>= %d chars) long.  Bye!\n\0".as_ptr()
                    as *const c_char,
                prog_name(),
                argv[1].len() as c_int,
            );
            sys::exit(1);
        }

        {
            let src = &argv[1];
            std::ptr::copy_nonoverlapping(
                src.as_ptr(),
                IN_FILE_NAME.as_mut_ptr() as *mut u8,
                src.len(),
            );
            IN_FILE_NAME[src.len()] = 0;
        }

        let mut in_file = sys::fopen(IN_FILE_NAME.as_ptr(), b"rb\0".as_ptr() as *const c_char);
        if in_file.is_null() {
            sys::fprintf(
                sys::stderr,
                b"%s: can't read `%s'\n\0".as_ptr() as *const c_char,
                prog_name(),
                IN_FILE_NAME.as_ptr(),
            );
            sys::exit(1);
        }

        let mut bs_in = bsOpenReadStream(in_file);
        sys::fprintf(
            sys::stderr,
            b"%s: searching for block boundaries ...\n\0".as_ptr() as *const c_char,
            prog_name(),
        );

        let mut bits_read: u64 = 0;
        let mut buff_hi: u32 = 0;
        let mut buff_lo: u32 = 0;
        let mut curr_block: i32 = 0;
        B_START[0] = 0;

        let mut rb_ctr: i32 = 0;

        loop {
            let b = bsGetBit(bs_in);
            bits_read += 1;
            if b == 2 {
                if bits_read >= B_START[curr_block as usize]
                    && (bits_read - B_START[curr_block as usize]) >= 40
                {
                    B_END[curr_block as usize] = bits_read - 1;
                    if curr_block > 0 {
                        sys::fprintf(
                            sys::stderr,
                            b"   block %d runs from %Lu to %Lu (incomplete)\n\0".as_ptr()
                                as *const c_char,
                            curr_block,
                            B_START[curr_block as usize],
                            B_END[curr_block as usize],
                        );
                    }
                } else {
                    curr_block -= 1;
                }
                break;
            }
            buff_hi = (buff_hi << 1) | (buff_lo >> 31);
            buff_lo = (buff_lo << 1) | ((b & 1) as u32);
            if (buff_hi & 0x0000ffff) == BLOCK_HEADER_HI && buff_lo == BLOCK_HEADER_LO
                || (buff_hi & 0x0000ffff) == BLOCK_ENDMARK_HI && buff_lo == BLOCK_ENDMARK_LO
            {
                if bits_read > 49 {
                    B_END[curr_block as usize] = bits_read - 49;
                } else {
                    B_END[curr_block as usize] = 0;
                }
                if curr_block > 0
                    && (B_END[curr_block as usize] - B_START[curr_block as usize]) >= 130
                {
                    sys::fprintf(
                        sys::stderr,
                        b"   block %d runs from %Lu to %Lu\n\0".as_ptr() as *const c_char,
                        rb_ctr + 1,
                        B_START[curr_block as usize],
                        B_END[curr_block as usize],
                    );
                    RB_START[rb_ctr as usize] = B_START[curr_block as usize];
                    RB_END[rb_ctr as usize] = B_END[curr_block as usize];
                    rb_ctr += 1;
                }
                if curr_block >= BZ_MAX_HANDLED_BLOCKS as i32 {
                    tooManyBlocks(BZ_MAX_HANDLED_BLOCKS as i32);
                }
                curr_block += 1;
                B_START[curr_block as usize] = bits_read;
            }
        }

        bsClose(bs_in);

        if rb_ctr < 1 {
            sys::fprintf(
                sys::stderr,
                b"%s: sorry, I couldn't find any block boundaries.\n\0".as_ptr()
                    as *const c_char,
                prog_name(),
            );
            sys::exit(1);
        }

        sys::fprintf(
            sys::stderr,
            b"%s: splitting into blocks\n\0".as_ptr() as *const c_char,
            prog_name(),
        );

        in_file = sys::fopen(IN_FILE_NAME.as_ptr(), b"rb\0".as_ptr() as *const c_char);
        if in_file.is_null() {
            sys::fprintf(
                sys::stderr,
                b"%s: can't open `%s'\n\0".as_ptr() as *const c_char,
                prog_name(),
                IN_FILE_NAME.as_ptr(),
            );
            sys::exit(1);
        }
        bs_in = bsOpenReadStream(in_file);

        let mut block_crc: u32 = 0;
        let mut bs_wr: *mut BitStream = std::ptr::null_mut();

        bits_read = 0;
        let mut out_file: *mut sys::FILE = std::ptr::null_mut();
        let mut wr_block: usize = 0;
        loop {
            let b = bsGetBit(bs_in);
            if b == 2 {
                break;
            }
            buff_hi = (buff_hi << 1) | (buff_lo >> 31);
            buff_lo = (buff_lo << 1) | ((b & 1) as u32);
            if bits_read == 47 + RB_START[wr_block] {
                block_crc = (buff_hi << 16) | (buff_lo >> 16);
            }

            if !out_file.is_null()
                && bits_read >= RB_START[wr_block]
                && bits_read <= RB_END[wr_block]
            {
                bsPutBit(bs_wr, b);
            }

            bits_read += 1;

            if bits_read == RB_END[wr_block] + 1 {
                if !out_file.is_null() {
                    bsPutUChar(bs_wr, 0x17);
                    bsPutUChar(bs_wr, 0x72);
                    bsPutUChar(bs_wr, 0x45);
                    bsPutUChar(bs_wr, 0x38);
                    bsPutUChar(bs_wr, 0x50);
                    bsPutUChar(bs_wr, 0x90);
                    bsPutUInt32(bs_wr, block_crc);
                    bsClose(bs_wr);
                    out_file = std::ptr::null_mut();
                }
                if wr_block >= rb_ctr as usize {
                    break;
                }
                wr_block += 1;
            } else if bits_read == RB_START[wr_block] {
                /* Create the output file name, correctly handling
                   leading paths. */
                let mut out_name: Vec<u8> = vec![0; BZ_MAX_FILENAME];
                let in_b: Vec<u8> = {
                    let mut v = Vec::new();
                    let mut i = 0;
                    while IN_FILE_NAME[i] != 0 {
                        v.push(IN_FILE_NAME[i] as u8);
                        i += 1;
                    }
                    v
                };
                /* strcpy outFileName = inFileName, then split at last '/' */
                out_name[..in_b.len()].copy_from_slice(&in_b);
                let split_pos = match in_b.iter().rposition(|&c| c == b'/') {
                    Some(p) => p + 1,
                    None => 0,
                };
                /* sprintf(split, "rec%5d", wrBlock+1) then ' '->'0' */
                let rec = format!("rec{:>5}", wr_block + 1).replace(' ', "0");
                let mut j = split_pos;
                for &ch in rec.as_bytes() {
                    out_name[j] = ch;
                    j += 1;
                }
                /* strcat(outFileName, inFileName + ofs) */
                for &ch in &in_b[split_pos..] {
                    out_name[j] = ch;
                    j += 1;
                }
                out_name[j] = 0;
                let out_str: Vec<u8> = out_name[..j].to_vec();

                let mut out_final = out_str.clone();
                if !endsInBz2(&out_final) {
                    out_final.extend_from_slice(b".bz2");
                }
                /* store back into the C array for fopen+messages */
                for (i, &ch) in out_final.iter().enumerate() {
                    OUT_FILE_NAME[i] = ch as c_char;
                }
                OUT_FILE_NAME[out_final.len()] = 0;

                sys::fprintf(
                    sys::stderr,
                    b"   writing block %d to `%s' ...\n\0".as_ptr() as *const c_char,
                    wr_block as c_int + 1,
                    OUT_FILE_NAME.as_ptr(),
                );

                out_file = sys::fopen(OUT_FILE_NAME.as_ptr(), b"wb\0".as_ptr() as *const c_char);
                if out_file.is_null() {
                    sys::fprintf(
                        sys::stderr,
                        b"%s: can't write `%s'\n\0".as_ptr() as *const c_char,
                        prog_name(),
                        OUT_FILE_NAME.as_ptr(),
                    );
                    sys::exit(1);
                }
                bs_wr = bsOpenWriteStream(out_file);
                bsPutUChar(bs_wr, BZ_HDR_B);
                bsPutUChar(bs_wr, BZ_HDR_Z);
                bsPutUChar(bs_wr, BZ_HDR_H);
                bsPutUChar(bs_wr, BZ_HDR_0 + 9);
                bsPutUChar(bs_wr, 0x31);
                bsPutUChar(bs_wr, 0x41);
                bsPutUChar(bs_wr, 0x59);
                bsPutUChar(bs_wr, 0x26);
                bsPutUChar(bs_wr, 0x53);
                bsPutUChar(bs_wr, 0x59);
            }
        }

        sys::fprintf(
            sys::stderr,
            b"%s: finished\n\0".as_ptr() as *const c_char,
            prog_name(),
        );
    }
}
