/*-------------------------------------------------------------*/
/*--- Common constants and types (bzlib.h + bzlib_private.h) ---*/
/*---                                             common.rs ---*/
/*-------------------------------------------------------------*/

/* This file is part of the Rust port of bzip2/libbzip2, a program
   and library for lossless, block-sorting data compression.

   bzip2/libbzip2 version 1.0.8 of 13 July 2019
   Copyright (C) 1996-2019 Julian Seward <jseward@acm.org>
   Rust port: a literal translation of the C sources.
*/

#![allow(non_camel_case_types)]
#![allow(dead_code)]

use std::os::raw::{c_char, c_int, c_uint, c_void};

pub const BZ_VERSION: &[u8] = b"1.0.8, 13-Jul-2019\0";

/*-- action codes for BZ2_bzCompress --*/
pub const BZ_RUN: i32 = 0;
pub const BZ_FLUSH: i32 = 1;
pub const BZ_FINISH: i32 = 2;

/*-- return codes --*/
pub const BZ_OK: i32 = 0;
pub const BZ_RUN_OK: i32 = 1;
pub const BZ_FLUSH_OK: i32 = 2;
pub const BZ_FINISH_OK: i32 = 3;
pub const BZ_STREAM_END: i32 = 4;
pub const BZ_SEQUENCE_ERROR: i32 = -1;
pub const BZ_PARAM_ERROR: i32 = -2;
pub const BZ_MEM_ERROR: i32 = -3;
pub const BZ_DATA_ERROR: i32 = -4;
pub const BZ_DATA_ERROR_MAGIC: i32 = -5;
pub const BZ_IO_ERROR: i32 = -6;
pub const BZ_UNEXPECTED_EOF: i32 = -7;
pub const BZ_OUTBUFF_FULL: i32 = -8;
pub const BZ_CONFIG_ERROR: i32 = -9;

pub const BZ_MAX_UNUSED: usize = 5000;

/*-- the public stream type; identical layout to the C struct --*/
pub type bzalloc_fn = Option<extern "C" fn(*mut c_void, c_int, c_int) -> *mut c_void>;
pub type bzfree_fn = Option<extern "C" fn(*mut c_void, *mut c_void)>;

#[repr(C)]
pub struct bz_stream {
    pub next_in: *mut c_char,
    pub avail_in: c_uint,
    pub total_in_lo32: c_uint,
    pub total_in_hi32: c_uint,

    pub next_out: *mut c_char,
    pub avail_out: c_uint,
    pub total_out_lo32: c_uint,
    pub total_out_hi32: c_uint,

    pub state: *mut c_void,

    pub bzalloc: bzalloc_fn,
    pub bzfree: bzfree_fn,
    pub opaque: *mut c_void,
}

impl bz_stream {
    pub fn zeroed() -> Self {
        bz_stream {
            next_in: std::ptr::null_mut(),
            avail_in: 0,
            total_in_lo32: 0,
            total_in_hi32: 0,
            next_out: std::ptr::null_mut(),
            avail_out: 0,
            total_out_lo32: 0,
            total_out_hi32: 0,
            state: std::ptr::null_mut(),
            bzalloc: None,
            bzfree: None,
            opaque: std::ptr::null_mut(),
        }
    }
}

pub type Char = i8;
pub type Bool = u8;
pub type UChar = u8;
pub type Int32 = i32;
pub type UInt32 = u32;
pub type Int16 = i16;
pub type UInt16 = u16;

pub const TRUE: Bool = 1;
pub const FALSE: Bool = 0;

/*-- Header bytes. --*/
pub const BZ_HDR_B: u8 = 0x42; /* 'B' */
pub const BZ_HDR_Z: u8 = 0x5a; /* 'Z' */
pub const BZ_HDR_H: u8 = 0x68; /* 'h' */
pub const BZ_HDR_0: u8 = 0x30; /* '0' */

/*-- Constants for the back end. --*/
pub const BZ_MAX_ALPHA_SIZE: usize = 258;
pub const BZ_MAX_CODE_LEN: usize = 23;

pub const BZ_RUNA: i32 = 0;
pub const BZ_RUNB: i32 = 1;

pub const BZ_N_GROUPS: usize = 6;
pub const BZ_G_SIZE: i32 = 50;
pub const BZ_N_ITERS: usize = 4;

pub const BZ_MAX_SELECTORS: usize = 2 + (900000 / BZ_G_SIZE as usize);

/*-- States and modes for compression. --*/
pub const BZ_M_IDLE: i32 = 1;
pub const BZ_M_RUNNING: i32 = 2;
pub const BZ_M_FLUSHING: i32 = 3;
pub const BZ_M_FINISHING: i32 = 4;

pub const BZ_S_OUTPUT: i32 = 1;
pub const BZ_S_INPUT: i32 = 2;

pub const BZ_N_RADIX: i32 = 2;
pub const BZ_N_QSORT: i32 = 12;
pub const BZ_N_SHELL: i32 = 18;
pub const BZ_N_OVERSHOOT: usize = (BZ_N_RADIX + BZ_N_QSORT + BZ_N_SHELL + 2) as usize;

/*-- states for decompression. --*/
pub const BZ_X_IDLE: i32 = 1;
pub const BZ_X_OUTPUT: i32 = 2;

pub const BZ_X_MAGIC_1: i32 = 10;
pub const BZ_X_MAGIC_2: i32 = 11;
pub const BZ_X_MAGIC_3: i32 = 12;
pub const BZ_X_MAGIC_4: i32 = 13;
pub const BZ_X_BLKHDR_1: i32 = 14;
pub const BZ_X_BLKHDR_2: i32 = 15;
pub const BZ_X_BLKHDR_3: i32 = 16;
pub const BZ_X_BLKHDR_4: i32 = 17;
pub const BZ_X_BLKHDR_5: i32 = 18;
pub const BZ_X_BLKHDR_6: i32 = 19;
pub const BZ_X_BCRC_1: i32 = 20;
pub const BZ_X_BCRC_2: i32 = 21;
pub const BZ_X_BCRC_3: i32 = 22;
pub const BZ_X_BCRC_4: i32 = 23;
pub const BZ_X_RANDBIT: i32 = 24;
pub const BZ_X_ORIGPTR_1: i32 = 25;
pub const BZ_X_ORIGPTR_2: i32 = 26;
pub const BZ_X_ORIGPTR_3: i32 = 27;
pub const BZ_X_MAPPING_1: i32 = 28;
pub const BZ_X_MAPPING_2: i32 = 29;
pub const BZ_X_SELECTOR_1: i32 = 30;
pub const BZ_X_SELECTOR_2: i32 = 31;
pub const BZ_X_SELECTOR_3: i32 = 32;
pub const BZ_X_CODING_1: i32 = 33;
pub const BZ_X_CODING_2: i32 = 34;
pub const BZ_X_CODING_3: i32 = 35;
pub const BZ_X_MTF_1: i32 = 36;
pub const BZ_X_MTF_2: i32 = 37;
pub const BZ_X_MTF_3: i32 = 38;
pub const BZ_X_MTF_4: i32 = 39;
pub const BZ_X_MTF_5: i32 = 40;
pub const BZ_X_MTF_6: i32 = 41;
pub const BZ_X_ENDHDR_2: i32 = 42;
pub const BZ_X_ENDHDR_3: i32 = 43;
pub const BZ_X_ENDHDR_4: i32 = 44;
pub const BZ_X_ENDHDR_5: i32 = 45;
pub const BZ_X_ENDHDR_6: i32 = 46;
pub const BZ_X_CCRC_1: i32 = 47;
pub const BZ_X_CCRC_2: i32 = 48;
pub const BZ_X_CCRC_3: i32 = 49;
pub const BZ_X_CCRC_4: i32 = 50;

/*-- Constants for the fast MTF decoder. --*/
pub const MTFA_SIZE: usize = 4096;
pub const MTFL_SIZE: usize = 16;

/*-- CRC helpers (BZ_UPDATE_CRC etc.) --*/
#[inline(always)]
pub fn bz_update_crc(crc: &mut u32, ch: u8) {
    *crc = (*crc << 8) ^ crate::tables::BZ2_CRC32_TABLE[(((*crc >> 24) as u8) ^ ch) as usize];
}

#[inline(always)]
pub fn bz_initialise_crc(crc: &mut u32) {
    *crc = 0xffffffff;
}

#[inline(always)]
pub fn bz_finalise_crc(crc: &mut u32) {
    *crc = !*crc;
}
