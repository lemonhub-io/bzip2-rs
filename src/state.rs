/*-------------------------------------------------------------*/
/*--- Internal stream state (EState / DState)               ---*/
/*---                              bzlib_private.h -> .rs   ---*/
/*-------------------------------------------------------------*/

/* This file is part of the Rust port of bzip2/libbzip2.
   Literal translation of the private state structures, plus the
   allocator plumbing (strm->bzalloc / strm->bzfree is part of the
   public contract, so all state memory goes through it). */

#![allow(non_snake_case)]
#![allow(dead_code)]

use crate::common::*;
use crate::sys;
use std::os::raw::{c_int, c_void};

/*---------------------------------------------------*/
/* The C allocator hooks.                            */
/*---------------------------------------------------*/
extern "C" fn default_bzalloc(_opaque: *mut c_void, items: c_int, size: c_int) -> *mut c_void {
    unsafe { sys::malloc((items as usize).wrapping_mul(size as usize)) }
}
extern "C" fn default_bzfree(_opaque: *mut c_void, addr: *mut c_void) {
    if !addr.is_null() {
        unsafe { sys::free(addr) }
    }
}

#[derive(Clone, Copy)]
pub struct BzAlloc {
    pub bzalloc: bzalloc_fn,
    pub bzfree: bzfree_fn,
    pub opaque: *mut c_void,
}

impl BzAlloc {
    /// # Safety
    /// `strm` must be a valid, live `bz_stream` pointer (or follow the same
    /// contract as the corresponding C API entry point).
    pub unsafe fn of(strm: *mut bz_stream) -> BzAlloc {
        let bzalloc = (*strm).bzalloc.unwrap_or(default_bzalloc);
        let bzfree = (*strm).bzfree.unwrap_or(default_bzfree);
        BzAlloc {
            bzalloc: Some(bzalloc),
            bzfree: Some(bzfree),
            opaque: (*strm).opaque,
        }
    }

    /* BZALLOC(nnn) : (strm->bzalloc)(strm->opaque,(nnn),1) */
    pub fn alloc_bytes(&self, nbytes: usize) -> *mut c_void {
        let f = self.bzalloc.unwrap();
        f(self.opaque, nbytes as c_int, 1)
    }

    /* allocate n items of size `size`, zero-initialised.  The C code
       uses malloc() (uninitialised) for these blocks; we clear them
       because every C read follows a write anyway, and uninitialised
       reads are not a thing we can safely express in Rust. */
    pub fn alloc_arr<T>(&self, items: usize, size: usize) -> *mut T {
        let f = self.bzalloc.unwrap();
        let p = f(self.opaque, items as c_int, size as c_int) as *mut T;
        if !p.is_null() {
            unsafe { std::ptr::write_bytes(p as *mut u8, 0, items * size) };
        }
        p
    }

    /* BZFREE(ppp) : (strm->bzfree)(strm->opaque,(ppp)) */
    pub fn free(&self, p: *mut c_void) {
        if !p.is_null() {
            let f = self.bzfree.unwrap();
            f(self.opaque, p);
        }
    }
}

/*---------------------------------------------------*/
/* A raw array allocated through the stream allocator. */
/* `at`/`set` bounds-check in debug builds only.       */
/*---------------------------------------------------*/
#[derive(Clone, Copy)]
pub struct Arr<T> {
    pub p: *mut T,
    pub len: usize, /* element count */
}

impl<T: Copy> Arr<T> {
    pub fn null() -> Self {
        Arr {
            p: std::ptr::null_mut(),
            len: 0,
        }
    }
    pub fn is_null(&self) -> bool {
        self.p.is_null()
    }
    #[inline(always)]
    pub fn at(&self, i: usize) -> T {
        debug_assert!(i < self.len);
        unsafe { *self.p.add(i) }
    }
    #[inline(always)]
    pub fn set(&mut self, i: usize, v: T) {
        debug_assert!(i < self.len);
        unsafe { *self.p.add(i) = v }
    }
}

/* little-endian byte/halfword views of a u32 array, matching the
   C code's array aliasing ((UChar*)arr2, (UInt16*)arr1, quadrant) */
impl Arr<u32> {
    #[inline(always)]
    pub fn get8(&self, i: usize) -> u8 {
        (self.at(i >> 2) >> ((i & 3) * 8)) as u8
    }
    #[inline(always)]
    pub fn set8(&mut self, i: usize, v: u8) {
        let s = (i & 3) * 8;
        let w = self.at(i >> 2);
        self.set(i >> 2, (w & !(0xffu32 << s)) | ((v as u32) << s));
    }
    /* u16 access at byte offset `bo` (must be even) */
    #[inline(always)]
    pub fn get16(&self, bo: usize) -> u16 {
        (self.at(bo >> 2) >> ((bo & 3) * 8)) as u16
    }
    #[inline(always)]
    pub fn set16(&mut self, bo: usize, v: u16) {
        let s = (bo & 3) * 8;
        let w = self.at(bo >> 2);
        self.set(bo >> 2, (w & !(0xffffu32 << s)) | ((v as u32) << s));
    }
}

/*---------------------------------------------------*/
/*-- Structure holding all the compression-side stuff. --*/
/*---------------------------------------------------*/
pub struct EState {
    /* pointer back to the struct bz_stream */
    pub strm: *mut bz_stream,

    /* mode this stream is in, and whether inputting */
    /* or outputting data */
    pub mode: i32,
    pub state: i32,

    /* remembers avail_in when flush/finish requested */
    pub avail_in_expect: u32,

    /* for doing the block sorting */
    pub arr1: Arr<u32>,
    pub arr2: Arr<u32>,
    pub ftab: Arr<u32>,
    pub orig_ptr: i32,

    /* for deciding when to use the fallback sorting algorithm */
    pub work_factor: i32,

    /* run-length-encoding of the input */
    pub state_in_ch: u32,
    pub state_in_len: i32,
    pub r_n_to_go: i32,
    pub r_t_pos: i32,

    /* input and output limits and current posns */
    pub nblock: i32,
    pub nblock_max: i32,
    pub num_z: i32,
    pub state_out_pos: i32,

    /* map of bytes used in block */
    pub n_in_use: i32,
    pub in_use: [Bool; 256],
    pub unseq_to_seq: [UChar; 256],

    /* the buffer for bit stream creation */
    pub bs_buff: u32,
    pub bs_live: i32,

    /* block and combined CRCs */
    pub block_crc: u32,
    pub combined_crc: u32,

    /* misc administratium */
    pub verbosity: i32,
    pub block_no: i32,
    pub block_size_100k: i32,

    /* stuff for coding the MTF values */
    pub n_mtf: i32,
    pub mtf_freq: [i32; BZ_MAX_ALPHA_SIZE],
    pub selector: [u8; BZ_MAX_SELECTORS],
    pub selector_mtf: [u8; BZ_MAX_SELECTORS],

    pub len: [[u8; BZ_MAX_ALPHA_SIZE]; BZ_N_GROUPS],
    pub code: [[i32; BZ_MAX_ALPHA_SIZE]; BZ_N_GROUPS],
    pub rfreq: [[i32; BZ_MAX_ALPHA_SIZE]; BZ_N_GROUPS],
    /* second dimension: only 3 needed; 4 makes index calculations faster */
    pub len_pack: [[u32; 4]; BZ_MAX_ALPHA_SIZE],
}

impl EState {
    /*-- the aliased views, exactly as C sets them up:
         s->block  = (UChar*)s->arr2
         s->mtfv   = (UInt16*)s->arr1
         s->ptr    = s->arr1
         s->zbits  = &((UChar*)s->arr2)[s->nblock]
         quadrant  = u16 view of arr2 at byte nblock+overshoot (aligned)
    --*/
    #[inline(always)]
    pub fn block_get(&self, i: usize) -> u8 {
        self.arr2.get8(i)
    }
    #[inline(always)]
    pub fn block_set(&mut self, i: usize, v: u8) {
        self.arr2.set8(i, v)
    }
    #[inline(always)]
    pub fn mtfv_get(&self, i: usize) -> u16 {
        self.arr1.get16(2 * i)
    }
    #[inline(always)]
    pub fn mtfv_set(&mut self, i: usize, v: u16) {
        self.arr1.set16(2 * i, v)
    }
    #[inline(always)]
    pub fn zbit_set(&mut self, i: usize, v: u8) {
        self.arr2.set8(self.nblock as usize + i, v)
    }
    #[inline(always)]
    pub fn zbit_get(&self, i: usize) -> u8 {
        self.arr2.get8(self.nblock as usize + i)
    }
}

/*---------------------------------------------------*/
/*-- Structure holding all the decompression-side stuff. --*/
/*---------------------------------------------------*/
pub struct DState {
    /* pointer back to the struct bz_stream */
    pub strm: *mut bz_stream,

    /* state indicator for this stream */
    pub state: i32,

    /* for doing the final run-length decoding */
    pub state_out_ch: u8,
    pub state_out_len: i32,
    pub block_randomised: Bool,
    pub r_n_to_go: i32,
    pub r_t_pos: i32,

    /* the buffer for bit stream reading */
    pub bs_buff: u32,
    pub bs_live: i32,

    /* misc administratium */
    pub block_size_100k: i32,
    pub small_decompress: Bool,
    pub curr_block_no: i32,
    pub verbosity: i32,

    /* for undoing the Burrows-Wheeler transform */
    pub orig_ptr: i32,
    pub t_pos: u32,
    pub k0: i32,
    pub unzftab: [i32; 256],
    pub nblock_used: i32,
    pub cftab: [i32; 257],
    pub cftab_copy: [i32; 257],

    /* for undoing the Burrows-Wheeler transform (FAST) */
    pub tt: Arr<u32>,

    /* for undoing the Burrows-Wheeler transform (SMALL) */
    pub ll16: Arr<u16>,
    pub ll4: Arr<u8>,

    /* stored and calculated CRCs */
    pub stored_block_crc: u32,
    pub stored_combined_crc: u32,
    pub calculated_block_crc: u32,
    pub calculated_combined_crc: u32,

    /* map of bytes used in block */
    pub n_in_use: i32,
    pub in_use: [Bool; 256],
    pub in_use16: [Bool; 16],
    pub seq_to_unseq: [UChar; 256],

    /* for decoding the MTF values */
    pub mtfa: [u8; MTFA_SIZE],
    pub mtfbase: [i32; 256 / MTFL_SIZE],
    pub selector: [u8; BZ_MAX_SELECTORS],
    pub selector_mtf: [u8; BZ_MAX_SELECTORS],
    pub len: [[u8; BZ_MAX_ALPHA_SIZE]; BZ_N_GROUPS],

    pub limit: [[i32; BZ_MAX_ALPHA_SIZE]; BZ_N_GROUPS],
    pub base: [[i32; BZ_MAX_ALPHA_SIZE]; BZ_N_GROUPS],
    pub perm: [[i32; BZ_MAX_ALPHA_SIZE]; BZ_N_GROUPS],
    pub min_lens: [i32; BZ_N_GROUPS],

    /* save area for scalars in the main decompress code */
    pub save_i: i32,
    pub save_j: i32,
    pub save_t: i32,
    pub save_alpha_size: i32,
    pub save_n_groups: i32,
    pub save_n_selectors: i32,
    pub save_eob: i32,
    pub save_group_no: i32,
    pub save_group_pos: i32,
    pub save_next_sym: i32,
    pub save_nblock_max: i32,
    pub save_nblock: i32,
    pub save_es: i32,
    pub save_n: i32,
    pub save_curr: i32,
    pub save_zt: i32,
    pub save_zn: i32,
    pub save_zvec: i32,
    pub save_zj: i32,
    pub save_g_sel: i32,
    pub save_g_minlen: i32,
    /* the C save_gLimit/save_gBase/save_gPerm are pointers into
       s->limit[s->gSel][0] etc; the group index gSel is itself
       saved, so we rederive the row from save_g_sel on demand. */
}

impl DState {
    /* SET_LL4 / GET_LL4 / SET_LL / GET_LL from bzlib_private.h */
    #[inline(always)]
    pub fn set_ll4(&mut self, i: usize, n: u32) {
        if (i & 1) == 0 {
            let v = (self.ll4.at(i >> 1) & 0xf0) | (n as u8);
            self.ll4.set(i >> 1, v);
        } else {
            let v = (self.ll4.at(i >> 1) & 0x0f) | ((n as u8) << 4);
            self.ll4.set(i >> 1, v);
        }
    }
    #[inline(always)]
    pub fn get_ll4(&self, i: usize) -> u32 {
        ((self.ll4.at(i >> 1) as u32) >> (((i as u32) << 2) & 0x4)) & 0xf
    }
    #[inline(always)]
    pub fn set_ll(&mut self, i: usize, n: u32) {
        self.ll16.set(i, (n & 0xffff) as u16);
        self.set_ll4(i, n >> 16);
    }
    #[inline(always)]
    pub fn get_ll(&self, i: usize) -> u32 {
        (self.ll16.at(i) as u32) | (self.get_ll4(i) << 16)
    }
}
