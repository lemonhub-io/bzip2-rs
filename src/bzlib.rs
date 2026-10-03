/*-------------------------------------------------------------*/
/*--- Library top-level functions.                          ---*/
/*---                                        bzlib.c -> .rs ---*/
/*-------------------------------------------------------------*/

/* This file is part of the Rust port of bzip2/libbzip2.
   Literal translation of bzlib.c. */

#![allow(non_snake_case)]
#![allow(non_camel_case_types)]

use crate::common::*;
use crate::compress::bz2_compress_block;
use crate::decompress::bz2_decompress;
use crate::state::{Arr, BzAlloc, DState, EState};
use crate::sys;
use std::os::raw::{c_char, c_int, c_uint, c_void};

pub type FILE = sys::FILE;

/*---------------------------------------------------*/
#[no_mangle]
pub extern "C" fn BZ2_bz__AssertH__fail(errcode: c_int) -> ! {
    unsafe {
        sys::fprintf(
            sys::stderr,
            b"\n\nbzip2/libbzip2: internal error number %d.\nThis is a bug in bzip2/libbzip2, %s.\nPlease report it to: bzip2-devel@sourceware.org.  If this happened\nwhen you were using some program which uses libbzip2 as a\ncomponent, you should also report this bug to the author(s)\nof that program.  Please make an effort to report this bug;\ntimely and accurate bug reports eventually lead to higher\nquality software.  Thanks.\n\n\0"
                .as_ptr() as *const c_char,
            errcode,
            BZ2_bzlibVersion(),
        );
        if errcode == 1007 {
            sys::fputs(b"\n*** A special note about internal error number 1007 ***\n\nExperience suggests that a common cause of i.e. 1007\nis unreliable memory or other hardware.  The 1007 assertion\njust happens to cross-check the results of huge numbers of\nmemory reads/writes, and so acts (unintendedly) as a stress\ntest of your memory system.\n\nI suggest the following: try compressing the file again,\npossibly monitoring progress in detail with the -vv flag.\n\n* If the error cannot be reproduced, and/or happens at different\n  points in compression, you may have a flaky memory system.\n  Try a memory-test program.  I have used Memtest86\n  (www.memtest86.com).  At the time of writing it is free (GPLd).\n  Memtest86 tests memory much more thorougly than your BIOSs\n  power-on test, and may find failures that the BIOS doesn't.\n\n* If the error can be repeatably reproduced, this is a bug in\n  bzip2, and I would very much like to hear about it.  Please\n  let me know, and, ideally, save a copy of the file causing the\n  problem -- without which I will be unable to investigate it.\n\n\0".as_ptr() as *const c_char, sys::stderr);
        }
        sys::exit(3);
    }
}

/* AssertH(cond, errcode): used throughout the translation; the C
   macro expands to `if (!(cond)) BZ2_bz__AssertH__fail(errcode)`. */
#[inline(always)]
pub fn assert_h_fail(errcode: i32) -> ! {
    BZ2_bz__AssertH__fail(errcode)
}

/*---------------------------------------------------*/
fn bz_config_ok() -> bool {
    /* mirrors the C sizeof checks; always true for this port */
    std::mem::size_of::<c_int>() == 4
        && std::mem::size_of::<i16>() == 2
        && std::mem::size_of::<u8>() == 1
}

/*---------------------------------------------------*/
fn prepare_new_block(s: &mut EState) {
    s.nblock = 0;
    s.num_z = 0;
    s.state_out_pos = 0;
    bz_initialise_crc(&mut s.block_crc);
    for i in 0..256 {
        s.in_use[i] = FALSE;
    }
    s.block_no += 1;
}

/*---------------------------------------------------*/
fn init_rl(s: &mut EState) {
    s.state_in_ch = 256;
    s.state_in_len = 0;
}

/* isempty_RL(s): !((s).state_in_ch < 256 && (s).state_in_len > 0) */
fn isempty_rl(s: &EState) -> bool {
    !(s.state_in_ch < 256 && s.state_in_len > 0)
}

/*---------------------------------------------------*/
#[no_mangle]
pub unsafe extern "C" fn BZ2_bzCompressInit(
    strm: *mut bz_stream,
    block_size_100k: c_int,
    verbosity: c_int,
    work_factor: c_int,
) -> c_int {
    if !bz_config_ok() {
        return BZ_CONFIG_ERROR;
    }

    if strm.is_null()
        || !(1..=9).contains(&block_size_100k)
        || !(0..=250).contains(&work_factor)
    {
        return BZ_PARAM_ERROR;
    }

    let mut work_factor = work_factor;
    if work_factor == 0 {
        work_factor = 30;
    }

    let alloc = BzAlloc::of(strm);
    (*strm).bzalloc = alloc.bzalloc;
    (*strm).bzfree = alloc.bzfree;

    let s_ptr = alloc.alloc_bytes(std::mem::size_of::<EState>()) as *mut EState;
    if s_ptr.is_null() {
        return BZ_MEM_ERROR;
    }
    std::ptr::write(s_ptr, std::mem::zeroed::<EState>());
    let s = &mut *s_ptr;
    s.strm = strm;

    s.arr1 = Arr::null();
    s.arr2 = Arr::null();
    s.ftab = Arr::null();

    let n = 100000usize * block_size_100k as usize;
    s.arr1 = Arr {
        p: alloc.alloc_arr::<u32>(n, 4),
        len: n,
    };
    s.arr2 = Arr {
        p: alloc.alloc_arr::<u32>(n + BZ_N_OVERSHOOT, 4),
        len: n + BZ_N_OVERSHOOT,
    };
    s.ftab = Arr {
        p: alloc.alloc_arr::<u32>(65537, 4),
        len: 65537,
    };

    if s.arr1.is_null() || s.arr2.is_null() || s.ftab.is_null() {
        if !s.arr1.is_null() {
            alloc.free(s.arr1.p as *mut c_void);
        }
        if !s.arr2.is_null() {
            alloc.free(s.arr2.p as *mut c_void);
        }
        if !s.ftab.is_null() {
            alloc.free(s.ftab.p as *mut c_void);
        }
        alloc.free(s_ptr as *mut c_void);
        return BZ_MEM_ERROR;
    }

    s.block_no = 0;
    s.state = BZ_S_INPUT;
    s.mode = BZ_M_RUNNING;
    s.combined_crc = 0;
    s.block_size_100k = block_size_100k;
    s.nblock_max = 100000 * block_size_100k - 19;
    s.verbosity = verbosity;
    s.work_factor = work_factor;

    /* s->block = arr2, s->mtfv/s->ptr = arr1, s->zbits = NULL:
       handled by the accessor methods (block_get/block_set,
       mtfv_get/mtfv_set, zbit_get/zbit_set). */
    (*strm).state = s_ptr as *mut c_void;
    (*strm).total_in_lo32 = 0;
    (*strm).total_in_hi32 = 0;
    (*strm).total_out_lo32 = 0;
    (*strm).total_out_hi32 = 0;
    init_rl(s);
    prepare_new_block(s);
    BZ_OK
}

/*---------------------------------------------------*/
fn add_pair_to_block(s: &mut EState) {
    let ch = s.state_in_ch as u8;
    for _i in 0..s.state_in_len {
        bz_update_crc(&mut s.block_crc, ch);
    }
    s.in_use[s.state_in_ch as usize] = TRUE;
    match s.state_in_len {
        1 => {
            let n = s.nblock as usize;
            s.block_set(n, ch);
            s.nblock += 1;
        }
        2 => {
            let n = s.nblock as usize;
            s.block_set(n, ch);
            s.block_set(n + 1, ch);
            s.nblock += 2;
        }
        3 => {
            let n = s.nblock as usize;
            s.block_set(n, ch);
            s.block_set(n + 1, ch);
            s.block_set(n + 2, ch);
            s.nblock += 3;
        }
        _ => {
            s.in_use[(s.state_in_len - 4) as usize] = TRUE;
            let n = s.nblock as usize;
            s.block_set(n, ch);
            s.block_set(n + 1, ch);
            s.block_set(n + 2, ch);
            s.block_set(n + 3, ch);
            s.block_set(n + 4, (s.state_in_len - 4) as u8);
            s.nblock += 5;
        }
    }
}

/*---------------------------------------------------*/
fn flush_rl(s: &mut EState) {
    if s.state_in_ch < 256 {
        add_pair_to_block(s);
    }
    init_rl(s);
}

/*---------------------------------------------------*/
/* ADD_CHAR_TO_BLOCK(zs,zchh0) */
#[inline(always)]
fn add_char_to_block(s: &mut EState, zchh0: u8) {
    let zchh = zchh0 as u32;
    /*-- fast track the common case --*/
    if zchh != s.state_in_ch && s.state_in_len == 1 {
        let ch = s.state_in_ch as u8;
        bz_update_crc(&mut s.block_crc, ch);
        s.in_use[s.state_in_ch as usize] = TRUE;
        let n = s.nblock as usize;
        s.block_set(n, ch);
        s.nblock += 1;
        s.state_in_ch = zchh;
    } else
    /*-- general, uncommon cases --*/
    if zchh != s.state_in_ch || s.state_in_len == 255 {
        if s.state_in_ch < 256 {
            add_pair_to_block(s);
        }
        s.state_in_ch = zchh;
        s.state_in_len = 1;
    } else {
        s.state_in_len += 1;
    }
}

/*---------------------------------------------------*/
unsafe fn strm_take_in(strm: *mut bz_stream) -> u8 {
    let ch = *((*strm).next_in as *const u8);
    (*strm).next_in = (*strm).next_in.add(1);
    (*strm).avail_in -= 1;
    (*strm).total_in_lo32 = (*strm).total_in_lo32.wrapping_add(1);
    if (*strm).total_in_lo32 == 0 {
        (*strm).total_in_hi32 = (*strm).total_in_hi32.wrapping_add(1);
    }
    ch
}

/*---------------------------------------------------*/
unsafe fn copy_input_until_stop(s: &mut EState) -> bool {
    let mut progress_in = false;
    let strm = s.strm;

    if s.mode == BZ_M_RUNNING {
        /*-- fast track the common case --*/
        loop {
            /*-- block full? --*/
            if s.nblock >= s.nblock_max {
                break;
            }
            /*-- no input? --*/
            if (*strm).avail_in == 0 {
                break;
            }
            progress_in = true;
            let ch = strm_take_in(strm);
            add_char_to_block(s, ch);
        }
    } else {
        /*-- general, uncommon case --*/
        loop {
            /*-- block full? --*/
            if s.nblock >= s.nblock_max {
                break;
            }
            /*-- no input? --*/
            if (*strm).avail_in == 0 {
                break;
            }
            /*-- flush/finish end? --*/
            if s.avail_in_expect == 0 {
                break;
            }
            progress_in = true;
            let ch = strm_take_in(strm);
            add_char_to_block(s, ch);
            s.avail_in_expect -= 1;
        }
    }
    progress_in
}

/*---------------------------------------------------*/
unsafe fn copy_output_until_stop(s: &mut EState) -> bool {
    let mut progress_out = false;
    let strm = s.strm;

    loop {
        /*-- no output space? --*/
        if (*strm).avail_out == 0 {
            break;
        }
        /*-- block done? --*/
        if s.state_out_pos >= s.num_z {
            break;
        }
        progress_out = true;
        *(*strm).next_out = s.zbit_get(s.state_out_pos as usize) as c_char;
        s.state_out_pos += 1;
        (*strm).avail_out -= 1;
        (*strm).next_out = (*strm).next_out.add(1);
        (*strm).total_out_lo32 = (*strm).total_out_lo32.wrapping_add(1);
        if (*strm).total_out_lo32 == 0 {
            (*strm).total_out_hi32 = (*strm).total_out_hi32.wrapping_add(1);
        }
    }

    progress_out
}

/*---------------------------------------------------*/
unsafe fn handle_compress(strm: *mut bz_stream) -> bool {
    let mut progress_in = false;
    let mut progress_out = false;
    let s = &mut *((*strm).state as *mut EState);

    loop {
        if s.state == BZ_S_OUTPUT {
            progress_out |= copy_output_until_stop(s);
            if s.state_out_pos < s.num_z {
                break;
            }
            if s.mode == BZ_M_FINISHING && s.avail_in_expect == 0 && isempty_rl(s) {
                break;
            }
            prepare_new_block(s);
            s.state = BZ_S_INPUT;
            if s.mode == BZ_M_FLUSHING && s.avail_in_expect == 0 && isempty_rl(s) {
                break;
            }
        }

        if s.state == BZ_S_INPUT {
            progress_in |= copy_input_until_stop(s);
            if s.mode != BZ_M_RUNNING && s.avail_in_expect == 0 {
                flush_rl(s);
                bz2_compress_block(s, s.mode == BZ_M_FINISHING);
                s.state = BZ_S_OUTPUT;
            } else if s.nblock >= s.nblock_max {
                bz2_compress_block(s, false);
                s.state = BZ_S_OUTPUT;
            } else if (*strm).avail_in == 0 {
                break;
            }
        }
    }

    progress_in || progress_out
}

/*---------------------------------------------------*/
#[no_mangle]
pub unsafe extern "C" fn BZ2_bzCompress(strm: *mut bz_stream, action: c_int) -> c_int {
    let progress: bool;
    if strm.is_null() {
        return BZ_PARAM_ERROR;
    }
    let s = (*strm).state as *mut EState;
    if s.is_null() {
        return BZ_PARAM_ERROR;
    }
    if (*s).strm != strm {
        return BZ_PARAM_ERROR;
    }

    /* preswitch: */
    let mut mode = (*s).mode;
    'preswitch: loop {
        match mode {
            BZ_M_IDLE => return BZ_SEQUENCE_ERROR,

            BZ_M_RUNNING => {
                if action == BZ_RUN {
                    progress = handle_compress(strm);
                    return if progress { BZ_RUN_OK } else { BZ_PARAM_ERROR };
                } else if action == BZ_FLUSH {
                    (*s).avail_in_expect = (*strm).avail_in;
                    (*s).mode = BZ_M_FLUSHING;
                    mode = BZ_M_FLUSHING;
                    continue 'preswitch;
                } else if action == BZ_FINISH {
                    (*s).avail_in_expect = (*strm).avail_in;
                    (*s).mode = BZ_M_FINISHING;
                    mode = BZ_M_FINISHING;
                    continue 'preswitch;
                } else {
                    return BZ_PARAM_ERROR;
                }
            }

            BZ_M_FLUSHING => {
                if action != BZ_FLUSH {
                    return BZ_SEQUENCE_ERROR;
                }
                if (*s).avail_in_expect != (*strm).avail_in {
                    return BZ_SEQUENCE_ERROR;
                }
                let _progress = handle_compress(strm);
                if (*s).avail_in_expect > 0
                    || !isempty_rl(&*s)
                    || (*s).state_out_pos < (*s).num_z
                {
                    return BZ_FLUSH_OK;
                }
                (*s).mode = BZ_M_RUNNING;
                return BZ_RUN_OK;
            }

            BZ_M_FINISHING => {
                if action != BZ_FINISH {
                    return BZ_SEQUENCE_ERROR;
                }
                if (*s).avail_in_expect != (*strm).avail_in {
                    return BZ_SEQUENCE_ERROR;
                }
                let progress = handle_compress(strm);
                if !progress {
                    return BZ_SEQUENCE_ERROR;
                }
                if (*s).avail_in_expect > 0
                    || !isempty_rl(&*s)
                    || (*s).state_out_pos < (*s).num_z
                {
                    return BZ_FINISH_OK;
                }
                (*s).mode = BZ_M_IDLE;
                return BZ_STREAM_END;
            }

            _ => return BZ_OK, /*--not reached--*/
        }
    }
}

/*---------------------------------------------------*/
#[no_mangle]
pub unsafe extern "C" fn BZ2_bzCompressEnd(strm: *mut bz_stream) -> c_int {
    if strm.is_null() {
        return BZ_PARAM_ERROR;
    }
    let s = (*strm).state as *mut EState;
    if s.is_null() {
        return BZ_PARAM_ERROR;
    }
    if (*s).strm != strm {
        return BZ_PARAM_ERROR;
    }

    let alloc = BzAlloc::of(strm);
    if !(*s).arr1.is_null() {
        alloc.free((*s).arr1.p as *mut c_void);
    }
    if !(*s).arr2.is_null() {
        alloc.free((*s).arr2.p as *mut c_void);
    }
    if !(*s).ftab.is_null() {
        alloc.free((*s).ftab.p as *mut c_void);
    }
    alloc.free((*strm).state);

    (*strm).state = std::ptr::null_mut();

    BZ_OK
}

/*---------------------------------------------------*/
/*--- Decompression stuff                         ---*/
/*---------------------------------------------------*/

/*---------------------------------------------------*/
#[no_mangle]
pub unsafe extern "C" fn BZ2_bzDecompressInit(
    strm: *mut bz_stream,
    verbosity: c_int,
    small: c_int,
) -> c_int {
    if !bz_config_ok() {
        return BZ_CONFIG_ERROR;
    }

    if strm.is_null() {
        return BZ_PARAM_ERROR;
    }
    if small != 0 && small != 1 {
        return BZ_PARAM_ERROR;
    }
    if !(0..=4).contains(&verbosity) {
        return BZ_PARAM_ERROR;
    }

    let alloc = BzAlloc::of(strm);
    (*strm).bzalloc = alloc.bzalloc;
    (*strm).bzfree = alloc.bzfree;

    let s_ptr = alloc.alloc_bytes(std::mem::size_of::<DState>()) as *mut DState;
    if s_ptr.is_null() {
        return BZ_MEM_ERROR;
    }
    std::ptr::write(s_ptr, std::mem::zeroed::<DState>());
    let s = &mut *s_ptr;
    s.strm = strm;
    (*strm).state = s_ptr as *mut c_void;
    s.state = BZ_X_MAGIC_1;
    s.bs_live = 0;
    s.bs_buff = 0;
    s.calculated_combined_crc = 0;
    (*strm).total_in_lo32 = 0;
    (*strm).total_in_hi32 = 0;
    (*strm).total_out_lo32 = 0;
    (*strm).total_out_hi32 = 0;
    s.small_decompress = small as Bool;
    s.ll4 = Arr::null();
    s.ll16 = Arr::null();
    s.tt = Arr::null();
    s.curr_block_no = 0;
    s.verbosity = verbosity;

    BZ_OK
}

/*---------------------------------------------------*/
/* Return  True iff data corruption is discovered.
   Returns False if there is no problem.
*/
unsafe fn un_rle_obuf_to_output_fast(s: &mut DState) -> bool {
    let mut k1: u8;
    let strm = s.strm;

    macro_rules! get_fast {
        ($dst:expr) => {{
            /* c_tPos is unsigned, hence test < 0 is pointless. */
            if s.t_pos >= (100000u32).wrapping_mul(s.block_size_100k as u32) {
                return true;
            }
            s.t_pos = s.tt.at(s.t_pos as usize);
            $dst = (s.t_pos & 0xff) as _;
            s.t_pos >>= 8;
        }};
    }
    macro_rules! rand_upd_mask {
        () => {{
            if s.r_n_to_go == 0 {
                s.r_n_to_go = crate::tables::BZ2_RNUMS[s.r_t_pos as usize] as i32;
                s.r_t_pos += 1;
                if s.r_t_pos == 512 {
                    s.r_t_pos = 0;
                }
            }
            s.r_n_to_go -= 1;
        }};
    }
    macro_rules! emit {
        ($ch:expr) => {{
            *((*strm).next_out as *mut u8) = $ch;
            bz_update_crc(&mut s.calculated_block_crc, $ch);
            (*strm).next_out = (*strm).next_out.add(1);
            (*strm).avail_out -= 1;
            (*strm).total_out_lo32 = (*strm).total_out_lo32.wrapping_add(1);
            if (*strm).total_out_lo32 == 0 {
                (*strm).total_out_hi32 = (*strm).total_out_hi32.wrapping_add(1);
            }
        }};
    }

    if s.block_randomised != 0 {
        loop {
            /* try to finish existing run */
            loop {
                if (*strm).avail_out == 0 {
                    return false;
                }
                if s.state_out_len == 0 {
                    break;
                }
                let ch = s.state_out_ch;
                emit!(ch);
                s.state_out_len -= 1;
            }

            /* can a new run be started? */
            if s.nblock_used == s.save_nblock + 1 {
                return false;
            }

            /* Only caused by corrupt data stream? */
            if s.nblock_used > s.save_nblock + 1 {
                return true;
            }

            s.state_out_len = 1;
            s.state_out_ch = s.k0 as u8;
            get_fast!(k1);
            rand_upd_mask!();
            k1 ^= if s.r_n_to_go == 1 { 1 } else { 0 };
            s.nblock_used += 1;
            if s.nblock_used == s.save_nblock + 1 {
                continue;
            }
            if k1 != s.k0 as u8 {
                s.k0 = k1 as i32;
                continue;
            };

            s.state_out_len = 2;
            get_fast!(k1);
            rand_upd_mask!();
            k1 ^= if s.r_n_to_go == 1 { 1 } else { 0 };
            s.nblock_used += 1;
            if s.nblock_used == s.save_nblock + 1 {
                continue;
            }
            if k1 != s.k0 as u8 {
                s.k0 = k1 as i32;
                continue;
            };

            s.state_out_len = 3;
            get_fast!(k1);
            rand_upd_mask!();
            k1 ^= if s.r_n_to_go == 1 { 1 } else { 0 };
            s.nblock_used += 1;
            if s.nblock_used == s.save_nblock + 1 {
                continue;
            }
            if k1 != s.k0 as u8 {
                s.k0 = k1 as i32;
                continue;
            };

            get_fast!(k1);
            rand_upd_mask!();
            k1 ^= if s.r_n_to_go == 1 { 1 } else { 0 };
            s.nblock_used += 1;
            s.state_out_len = k1 as i32 + 4;
            let mut kk: i32;
            get_fast!(kk);
            rand_upd_mask!();
            kk ^= if s.r_n_to_go == 1 { 1 } else { 0 };
            s.k0 = kk;
            s.nblock_used += 1;
        }
    } else {
        /* restore */
        let mut c_calculated_block_crc = s.calculated_block_crc;
        let mut c_state_out_ch = s.state_out_ch;
        let mut c_state_out_len = s.state_out_len;
        let mut c_nblock_used = s.nblock_used;
        let mut c_k0 = s.k0;
        let c_tt = s.tt;
        let mut c_t_pos = s.t_pos;
        let mut cs_next_out = (*strm).next_out;
        let mut cs_avail_out = (*strm).avail_out;
        let ro_block_size_100k = s.block_size_100k;
        /* end restore */

        let avail_out_init = cs_avail_out;
        let s_save_nblock_pp = s.save_nblock + 1;
        let total_out_lo32_old: u32;

        macro_rules! get_fast_c {
            ($dst:expr) => {{
                if c_t_pos >= (100000u32).wrapping_mul(ro_block_size_100k as u32) {
                    return true;
                }
                c_t_pos = c_tt.at(c_t_pos as usize);
                $dst = (c_t_pos & 0xff) as _;
                c_t_pos >>= 8;
            }};
        }
        macro_rules! emit_c {
            ($ch:expr) => {{
                *(cs_next_out as *mut u8) = $ch;
                bz_update_crc(&mut c_calculated_block_crc, $ch);
                cs_next_out = cs_next_out.add(1);
                cs_avail_out -= 1;
            }};
        }
        macro_rules! return_notr {
            () => {{
                total_out_lo32_old = (*strm).total_out_lo32;
                (*strm).total_out_lo32 = (*strm)
                    .total_out_lo32
                    .wrapping_add(avail_out_init - cs_avail_out);
                if (*strm).total_out_lo32 < total_out_lo32_old {
                    (*strm).total_out_hi32 =
                        (*strm).total_out_hi32.wrapping_add(1);
                }

                /* save */
                s.calculated_block_crc = c_calculated_block_crc;
                s.state_out_ch = c_state_out_ch;
                s.state_out_len = c_state_out_len;
                s.nblock_used = c_nblock_used;
                s.k0 = c_k0;
                s.tt = c_tt;
                s.t_pos = c_t_pos;
                (*strm).next_out = cs_next_out;
                (*strm).avail_out = cs_avail_out;
                /* end save */
                return false;
            }};
        }
        'outer: loop {
            /* try to finish existing run */
            let mut do_eq1 = c_state_out_len > 0;
            if do_eq1 {
                loop {
                    if cs_avail_out == 0 {
                        return_notr!();
                    }
                    if c_state_out_len == 1 {
                        break;
                    }
                    emit_c!(c_state_out_ch);
                    c_state_out_len -= 1;
                }
            }
            /* `goto s_state_out_len_eq_one` = emit once, then fall into
               the run-detection tail; emulate as this inner loop. */
            'eq1_a: loop {
                /* s_state_out_len_eq_one: */
                if do_eq1 {
                    if cs_avail_out == 0 {
                        c_state_out_len = 1;
                        return_notr!();
                    };
                    emit_c!(c_state_out_ch);
                }
                do_eq1 = true;

                /* Only caused by corrupt data stream? */
                if c_nblock_used > s_save_nblock_pp {
                    return true;
                }

                /* can a new run be started? */
                if c_nblock_used == s_save_nblock_pp {
                    c_state_out_len = 0;
                    return_notr!();
                };
                c_state_out_ch = c_k0 as u8;
                get_fast_c!(k1);
                c_nblock_used += 1;
                if k1 != c_k0 as u8 {
                    c_k0 = k1 as i32;
                    continue 'eq1_a;
                };
                if c_nblock_used == s_save_nblock_pp {
                    continue 'eq1_a;
                }

                c_state_out_len = 2;
                get_fast_c!(k1);
                c_nblock_used += 1;
                if c_nblock_used == s_save_nblock_pp {
                    break 'eq1_a;
                }
                if k1 != c_k0 as u8 {
                    c_k0 = k1 as i32;
                    break 'eq1_a;
                };

                c_state_out_len = 3;
                get_fast_c!(k1);
                c_nblock_used += 1;
                if c_nblock_used == s_save_nblock_pp {
                    break 'eq1_a;
                }
                if k1 != c_k0 as u8 {
                    c_k0 = k1 as i32;
                    break 'eq1_a;
                };

                get_fast_c!(k1);
                c_nblock_used += 1;
                c_state_out_len = k1 as i32 + 4;
                get_fast_c!(c_k0);
                c_nblock_used += 1;
                break 'eq1_a;
            }
            continue 'outer;
        }
    }
}

/*---------------------------------------------------*/
#[inline]
pub fn bz2_index_into_f(indx: i32, cftab: &[i32; 257]) -> i32 {
    let mut nb = 0i32;
    let mut na = 256i32;
    loop {
        let mid = (nb + na) >> 1;
        if indx >= cftab[mid as usize] {
            nb = mid;
        } else {
            na = mid;
        }
        if na - nb == 1 {
            break;
        }
    }
    nb
}

/*---------------------------------------------------*/
/* Return  True iff data corruption is discovered.
   Returns False if there is no problem.
*/
unsafe fn un_rle_obuf_to_output_small(s: &mut DState) -> bool {
    let mut k1: u8;
    let strm = s.strm;

    macro_rules! get_small {
        ($dst:expr) => {{
            /* c_tPos is unsigned, hence test < 0 is pointless. */
            if s.t_pos >= (100000u32).wrapping_mul(s.block_size_100k as u32) {
                return true;
            }
            $dst = bz2_index_into_f(s.t_pos as i32, &s.cftab) as _;
            s.t_pos = s.get_ll(s.t_pos as usize);
        }};
    }
    macro_rules! rand_upd_mask {
        () => {{
            if s.r_n_to_go == 0 {
                s.r_n_to_go = crate::tables::BZ2_RNUMS[s.r_t_pos as usize] as i32;
                s.r_t_pos += 1;
                if s.r_t_pos == 512 {
                    s.r_t_pos = 0;
                }
            }
            s.r_n_to_go -= 1;
        }};
    }
    macro_rules! emit {
        ($ch:expr) => {{
            *((*strm).next_out as *mut u8) = $ch;
            bz_update_crc(&mut s.calculated_block_crc, $ch);
            (*strm).next_out = (*strm).next_out.add(1);
            (*strm).avail_out -= 1;
            (*strm).total_out_lo32 = (*strm).total_out_lo32.wrapping_add(1);
            if (*strm).total_out_lo32 == 0 {
                (*strm).total_out_hi32 = (*strm).total_out_hi32.wrapping_add(1);
            }
        }};
    }

    if s.block_randomised != 0 {
        loop {
            /* try to finish existing run */
            loop {
                if (*strm).avail_out == 0 {
                    return false;
                }
                if s.state_out_len == 0 {
                    break;
                }
                let ch = s.state_out_ch;
                emit!(ch);
                s.state_out_len -= 1;
            }

            /* can a new run be started? */
            if s.nblock_used == s.save_nblock + 1 {
                return false;
            }

            /* Only caused by corrupt data stream? */
            if s.nblock_used > s.save_nblock + 1 {
                return true;
            }

            s.state_out_len = 1;
            s.state_out_ch = s.k0 as u8;
            get_small!(k1);
            rand_upd_mask!();
            k1 ^= if s.r_n_to_go == 1 { 1 } else { 0 };
            s.nblock_used += 1;
            if s.nblock_used == s.save_nblock + 1 {
                continue;
            }
            if k1 != s.k0 as u8 {
                s.k0 = k1 as i32;
                continue;
            };

            s.state_out_len = 2;
            get_small!(k1);
            rand_upd_mask!();
            k1 ^= if s.r_n_to_go == 1 { 1 } else { 0 };
            s.nblock_used += 1;
            if s.nblock_used == s.save_nblock + 1 {
                continue;
            }
            if k1 != s.k0 as u8 {
                s.k0 = k1 as i32;
                continue;
            };

            s.state_out_len = 3;
            get_small!(k1);
            rand_upd_mask!();
            k1 ^= if s.r_n_to_go == 1 { 1 } else { 0 };
            s.nblock_used += 1;
            if s.nblock_used == s.save_nblock + 1 {
                continue;
            }
            if k1 != s.k0 as u8 {
                s.k0 = k1 as i32;
                continue;
            };

            get_small!(k1);
            rand_upd_mask!();
            k1 ^= if s.r_n_to_go == 1 { 1 } else { 0 };
            s.nblock_used += 1;
            s.state_out_len = k1 as i32 + 4;
            get_small!(s.k0);
            rand_upd_mask!();
            s.k0 ^= if s.r_n_to_go == 1 { 1 } else { 0 };
            s.nblock_used += 1;
        }
    } else {
        loop {
            /* try to finish existing run */
            loop {
                if (*strm).avail_out == 0 {
                    return false;
                }
                if s.state_out_len == 0 {
                    break;
                }
                let ch = s.state_out_ch;
                emit!(ch);
                s.state_out_len -= 1;
            }

            /* can a new run be started? */
            if s.nblock_used == s.save_nblock + 1 {
                return false;
            }

            /* Only caused by corrupt data stream? */
            if s.nblock_used > s.save_nblock + 1 {
                return true;
            }

            s.state_out_len = 1;
            s.state_out_ch = s.k0 as u8;
            get_small!(k1);
            s.nblock_used += 1;
            if s.nblock_used == s.save_nblock + 1 {
                continue;
            }
            if k1 != s.k0 as u8 {
                s.k0 = k1 as i32;
                continue;
            };

            s.state_out_len = 2;
            get_small!(k1);
            s.nblock_used += 1;
            if s.nblock_used == s.save_nblock + 1 {
                continue;
            }
            if k1 != s.k0 as u8 {
                s.k0 = k1 as i32;
                continue;
            };

            s.state_out_len = 3;
            get_small!(k1);
            s.nblock_used += 1;
            if s.nblock_used == s.save_nblock + 1 {
                continue;
            }
            if k1 != s.k0 as u8 {
                s.k0 = k1 as i32;
                continue;
            };

            get_small!(k1);
            s.nblock_used += 1;
            s.state_out_len = k1 as i32 + 4;
            get_small!(s.k0);
            s.nblock_used += 1;
        }
    }
}

/*---------------------------------------------------*/
#[no_mangle]
pub unsafe extern "C" fn BZ2_bzDecompress(strm: *mut bz_stream) -> c_int {
    if strm.is_null() {
        return BZ_PARAM_ERROR;
    }
    let s = (*strm).state as *mut DState;
    if s.is_null() {
        return BZ_PARAM_ERROR;
    }
    if (*s).strm != strm {
        return BZ_PARAM_ERROR;
    }

    loop {
        if (*s).state == BZ_X_IDLE {
            return BZ_SEQUENCE_ERROR;
        }
        if (*s).state == BZ_X_OUTPUT {
            let corrupt = if (*s).small_decompress != 0 {
                un_rle_obuf_to_output_small(&mut *s)
            } else {
                un_rle_obuf_to_output_fast(&mut *s)
            };
            if corrupt {
                return BZ_DATA_ERROR;
            }
            if (*s).nblock_used == (*s).save_nblock + 1 && (*s).state_out_len == 0 {
                bz_finalise_crc(&mut (*s).calculated_block_crc);
                if (*s).verbosity >= 3 {
                    let msg = format!(
                        " {{0x{:08x}, 0x{:08x}}}",
                        (*s).stored_block_crc, (*s).calculated_block_crc
                    );
                    sys::fputs(msg.as_ptr() as *const c_char, sys::stderr);
                }
                if (*s).verbosity >= 2 {
                    sys::fputs(b"]\0".as_ptr() as *const c_char, sys::stderr);
                }
                if (*s).calculated_block_crc != (*s).stored_block_crc {
                    return BZ_DATA_ERROR;
                }
                (*s).calculated_combined_crc =
                    (*s).calculated_combined_crc.rotate_left(1);
                (*s).calculated_combined_crc ^= (*s).calculated_block_crc;
                (*s).state = BZ_X_BLKHDR_1;
            } else {
                return BZ_OK;
            }
        }
        if (*s).state >= BZ_X_MAGIC_1 {
            let r = bz2_decompress(&mut *s);
            if r == BZ_STREAM_END {
                if (*s).verbosity >= 3 {
                    let msg = format!(
                        "\n    combined CRCs: stored = 0x{:08x}, computed = 0x{:08x}",
                        (*s).stored_combined_crc, (*s).calculated_combined_crc
                    );
                    sys::fputs(msg.as_ptr() as *const c_char, sys::stderr);
                }
                if (*s).calculated_combined_crc != (*s).stored_combined_crc {
                    return BZ_DATA_ERROR;
                }
                return r;
            }
            if (*s).state != BZ_X_OUTPUT {
                return r;
            }
        }
    }
}

/*---------------------------------------------------*/
#[no_mangle]
pub unsafe extern "C" fn BZ2_bzDecompressEnd(strm: *mut bz_stream) -> c_int {
    if strm.is_null() {
        return BZ_PARAM_ERROR;
    }
    let s = (*strm).state as *mut DState;
    if s.is_null() {
        return BZ_PARAM_ERROR;
    }
    if (*s).strm != strm {
        return BZ_PARAM_ERROR;
    }

    let alloc = BzAlloc::of(strm);
    if !(*s).tt.is_null() {
        alloc.free((*s).tt.p as *mut c_void);
    }
    if !(*s).ll16.is_null() {
        alloc.free((*s).ll16.p as *mut c_void);
    }
    if !(*s).ll4.is_null() {
        alloc.free((*s).ll4.p as *mut c_void);
    }

    alloc.free((*strm).state);
    (*strm).state = std::ptr::null_mut();

    BZ_OK
}

/*---------------------------------------------------*/
/*--- File I/O stuff                              ---*/
/*---------------------------------------------------*/

pub type BZFILE = c_void;

#[repr(C)]
pub struct bzFile {
    pub handle: *mut FILE,
    pub buf: [c_char; BZ_MAX_UNUSED],
    pub buf_n: c_int,
    pub writing: Bool,
    pub strm: bz_stream,
    pub last_err: c_int,
    pub initialised_ok: Bool,
}

/*---------------------------------------------*/
unsafe fn myfeof(f: *mut FILE) -> bool {
    let c = sys::fgetc(f);
    if c == sys::EOF {
        return true;
    }
    sys::ungetc(c, f);
    false
}

/*---------------------------------------------*/
#[no_mangle]
pub unsafe extern "C" fn BZ2_bzWriteOpen(
    bzerror: *mut c_int,
    f: *mut FILE,
    block_size_100k: c_int,
    verbosity: c_int,
    work_factor: c_int,
) -> *mut BZFILE {
    macro_rules! seterr {
        ($e:expr, $bzf:expr) => {{
            if !bzerror.is_null() {
                *bzerror = $e;
            }
            if !$bzf.is_null() {
                (*$bzf).last_err = $e;
            }
        }};
    }

    let mut bzf: *mut bzFile = std::ptr::null_mut();
    seterr!(BZ_OK, bzf);

    if f.is_null()
        || !(1..=9).contains(&block_size_100k)
        || !(0..=250).contains(&work_factor)
        || !(0..=4).contains(&verbosity)
    {
        seterr!(BZ_PARAM_ERROR, bzf);
        return std::ptr::null_mut();
    }

    if sys::ferror(f) != 0 {
        seterr!(BZ_IO_ERROR, bzf);
        return std::ptr::null_mut();
    }

    bzf = sys::malloc(std::mem::size_of::<bzFile>()) as *mut bzFile;
    if bzf.is_null() {
        seterr!(BZ_MEM_ERROR, bzf);
        return std::ptr::null_mut();
    }
    std::ptr::write(bzf, std::mem::zeroed::<bzFile>());

    seterr!(BZ_OK, bzf);
    (*bzf).initialised_ok = FALSE;
    (*bzf).buf_n = 0;
    (*bzf).handle = f;
    (*bzf).writing = TRUE;
    (*bzf).strm.bzalloc = None;
    (*bzf).strm.bzfree = None;
    (*bzf).strm.opaque = std::ptr::null_mut();

    let mut work_factor = work_factor;
    if work_factor == 0 {
        work_factor = 30;
    }
    let ret = BZ2_bzCompressInit(&mut (*bzf).strm, block_size_100k, verbosity, work_factor);
    if ret != BZ_OK {
        seterr!(ret, bzf);
        sys::free(bzf as *mut c_void);
        return std::ptr::null_mut();
    }

    (*bzf).strm.avail_in = 0;
    (*bzf).initialised_ok = TRUE;
    bzf as *mut BZFILE
}

/*---------------------------------------------------*/
#[no_mangle]
pub unsafe extern "C" fn BZ2_bzWrite(
    bzerror: *mut c_int,
    b: *mut BZFILE,
    buf: *mut c_void,
    len: c_int,
) {
    macro_rules! seterr {
        ($e:expr, $bzf:expr) => {{
            if !bzerror.is_null() {
                *bzerror = $e;
            }
            if !$bzf.is_null() {
                (*$bzf).last_err = $e;
            }
        }};
    }

    let bzf = b as *mut bzFile;
    seterr!(BZ_OK, bzf);
    if bzf.is_null() || buf.is_null() || len < 0 {
        seterr!(BZ_PARAM_ERROR, bzf);
        return;
    }
    if (*bzf).writing == 0 {
        seterr!(BZ_SEQUENCE_ERROR, bzf);
        return;
    }
    if sys::ferror((*bzf).handle) != 0 {
        seterr!(BZ_IO_ERROR, bzf);
        return;
    }

    if len == 0 {
        seterr!(BZ_OK, bzf);
        return;
    }

    (*bzf).strm.avail_in = len as c_uint;
    (*bzf).strm.next_in = buf as *mut c_char;

    loop {
        (*bzf).strm.avail_out = BZ_MAX_UNUSED as c_uint;
        (*bzf).strm.next_out = (*bzf).buf.as_mut_ptr();
        let ret = BZ2_bzCompress(&mut (*bzf).strm, BZ_RUN);
        if ret != BZ_RUN_OK {
            seterr!(ret, bzf);
            return;
        }

        if (*bzf).strm.avail_out < BZ_MAX_UNUSED as c_uint {
            let n = BZ_MAX_UNUSED - (*bzf).strm.avail_out as usize;
            let n2 = sys::fwrite(
                (*bzf).buf.as_ptr() as *const c_void,
                1,
                n,
                (*bzf).handle,
            );
            if n != n2 || sys::ferror((*bzf).handle) != 0 {
                seterr!(BZ_IO_ERROR, bzf);
                return;
            }
        }

        if (*bzf).strm.avail_in == 0 {
            seterr!(BZ_OK, bzf);
            return;
        }
    }
}

/*---------------------------------------------------*/
#[no_mangle]
pub unsafe extern "C" fn BZ2_bzWriteClose(
    bzerror: *mut c_int,
    b: *mut BZFILE,
    abandon: c_int,
    nbytes_in: *mut c_uint,
    nbytes_out: *mut c_uint,
) {
    BZ2_bzWriteClose64(
        bzerror,
        b,
        abandon,
        nbytes_in,
        std::ptr::null_mut(),
        nbytes_out,
        std::ptr::null_mut(),
    );
}

/*---------------------------------------------------*/
#[no_mangle]
pub unsafe extern "C" fn BZ2_bzWriteClose64(
    bzerror: *mut c_int,
    b: *mut BZFILE,
    abandon: c_int,
    nbytes_in_lo32: *mut c_uint,
    nbytes_in_hi32: *mut c_uint,
    nbytes_out_lo32: *mut c_uint,
    nbytes_out_hi32: *mut c_uint,
) {
    macro_rules! seterr {
        ($e:expr, $bzf:expr) => {{
            if !bzerror.is_null() {
                *bzerror = $e;
            }
            if !$bzf.is_null() {
                (*$bzf).last_err = $e;
            }
        }};
    }

    let bzf = b as *mut bzFile;
    if bzf.is_null() {
        seterr!(BZ_OK, bzf);
        return;
    }
    if (*bzf).writing == 0 {
        seterr!(BZ_SEQUENCE_ERROR, bzf);
        return;
    }
    if sys::ferror((*bzf).handle) != 0 {
        seterr!(BZ_IO_ERROR, bzf);
        return;
    }

    if !nbytes_in_lo32.is_null() {
        *nbytes_in_lo32 = 0;
    }
    if !nbytes_in_hi32.is_null() {
        *nbytes_in_hi32 = 0;
    }
    if !nbytes_out_lo32.is_null() {
        *nbytes_out_lo32 = 0;
    }
    if !nbytes_out_hi32.is_null() {
        *nbytes_out_hi32 = 0;
    }

    if abandon == 0 && (*bzf).last_err == BZ_OK {
        loop {
            (*bzf).strm.avail_out = BZ_MAX_UNUSED as c_uint;
            (*bzf).strm.next_out = (*bzf).buf.as_mut_ptr();
            let ret = BZ2_bzCompress(&mut (*bzf).strm, BZ_FINISH);
            if ret != BZ_FINISH_OK && ret != BZ_STREAM_END {
                seterr!(ret, bzf);
                return;
            }

            if (*bzf).strm.avail_out < BZ_MAX_UNUSED as c_uint {
                let n = BZ_MAX_UNUSED - (*bzf).strm.avail_out as usize;
                let n2 = sys::fwrite(
                    (*bzf).buf.as_ptr() as *const c_void,
                    1,
                    n,
                    (*bzf).handle,
                );
                if n != n2 || sys::ferror((*bzf).handle) != 0 {
                    seterr!(BZ_IO_ERROR, bzf);
                    return;
                }
            }

            if ret == BZ_STREAM_END {
                break;
            }
        }
    }

    if abandon == 0 && sys::ferror((*bzf).handle) == 0 {
        sys::fflush((*bzf).handle);
        if sys::ferror((*bzf).handle) != 0 {
            seterr!(BZ_IO_ERROR, bzf);
            return;
        }
    }

    if !nbytes_in_lo32.is_null() {
        *nbytes_in_lo32 = (*bzf).strm.total_in_lo32;
    }
    if !nbytes_in_hi32.is_null() {
        *nbytes_in_hi32 = (*bzf).strm.total_in_hi32;
    }
    if !nbytes_out_lo32.is_null() {
        *nbytes_out_lo32 = (*bzf).strm.total_out_lo32;
    }
    if !nbytes_out_hi32.is_null() {
        *nbytes_out_hi32 = (*bzf).strm.total_out_hi32;
    }

    seterr!(BZ_OK, bzf);
    BZ2_bzCompressEnd(&mut (*bzf).strm);
    sys::free(bzf as *mut c_void);
}

/*---------------------------------------------------*/
#[no_mangle]
pub unsafe extern "C" fn BZ2_bzReadOpen(
    bzerror: *mut c_int,
    f: *mut FILE,
    verbosity: c_int,
    small: c_int,
    unused: *mut c_void,
    n_unused: c_int,
) -> *mut BZFILE {
    macro_rules! seterr {
        ($e:expr, $bzf:expr) => {{
            if !bzerror.is_null() {
                *bzerror = $e;
            }
            if !$bzf.is_null() {
                (*$bzf).last_err = $e;
            }
        }};
    }

    let mut bzf: *mut bzFile = std::ptr::null_mut();
    seterr!(BZ_OK, bzf);

    if f.is_null()
        || (small != 0 && small != 1)
        || !(0..=4).contains(&verbosity)
        || (unused.is_null() && n_unused != 0)
        || (!unused.is_null() && !(0..=BZ_MAX_UNUSED as c_int).contains(&n_unused))
    {
        seterr!(BZ_PARAM_ERROR, bzf);
        return std::ptr::null_mut();
    }

    if sys::ferror(f) != 0 {
        seterr!(BZ_IO_ERROR, bzf);
        return std::ptr::null_mut();
    }

    bzf = sys::malloc(std::mem::size_of::<bzFile>()) as *mut bzFile;
    if bzf.is_null() {
        seterr!(BZ_MEM_ERROR, bzf);
        return std::ptr::null_mut();
    }
    std::ptr::write(bzf, std::mem::zeroed::<bzFile>());

    seterr!(BZ_OK, bzf);

    (*bzf).initialised_ok = FALSE;
    (*bzf).handle = f;
    (*bzf).buf_n = 0;
    (*bzf).writing = FALSE;
    (*bzf).strm.bzalloc = None;
    (*bzf).strm.bzfree = None;
    (*bzf).strm.opaque = std::ptr::null_mut();

    let mut unused_p = unused as *mut u8;
    let mut n_unused = n_unused;
    while n_unused > 0 {
        (*bzf).buf[(*bzf).buf_n as usize] = *unused_p as c_char;
        (*bzf).buf_n += 1;
        unused_p = unused_p.add(1);
        n_unused -= 1;
    }

    let ret = BZ2_bzDecompressInit(&mut (*bzf).strm, verbosity, small);
    if ret != BZ_OK {
        seterr!(ret, bzf);
        sys::free(bzf as *mut c_void);
        return std::ptr::null_mut();
    }

    (*bzf).strm.avail_in = (*bzf).buf_n as c_uint;
    (*bzf).strm.next_in = (*bzf).buf.as_mut_ptr();

    (*bzf).initialised_ok = TRUE;
    bzf as *mut BZFILE
}

/*---------------------------------------------------*/
#[no_mangle]
pub unsafe extern "C" fn BZ2_bzReadClose(bzerror: *mut c_int, b: *mut BZFILE) {
    macro_rules! seterr {
        ($e:expr, $bzf:expr) => {{
            if !bzerror.is_null() {
                *bzerror = $e;
            }
            if !$bzf.is_null() {
                (*$bzf).last_err = $e;
            }
        }};
    }

    let bzf = b as *mut bzFile;
    seterr!(BZ_OK, bzf);
    if bzf.is_null() {
        seterr!(BZ_OK, bzf);
        return;
    }

    if (*bzf).writing != 0 {
        seterr!(BZ_SEQUENCE_ERROR, bzf);
        return;
    }

    if (*bzf).initialised_ok != 0 {
        BZ2_bzDecompressEnd(&mut (*bzf).strm);
    }
    sys::free(bzf as *mut c_void);
}

/*---------------------------------------------------*/
#[no_mangle]
pub unsafe extern "C" fn BZ2_bzRead(
    bzerror: *mut c_int,
    b: *mut BZFILE,
    buf: *mut c_void,
    len: c_int,
) -> c_int {
    macro_rules! seterr {
        ($e:expr, $bzf:expr) => {{
            if !bzerror.is_null() {
                *bzerror = $e;
            }
            if !$bzf.is_null() {
                (*$bzf).last_err = $e;
            }
        }};
    }

    let bzf = b as *mut bzFile;
    seterr!(BZ_OK, bzf);

    if bzf.is_null() || buf.is_null() || len < 0 {
        seterr!(BZ_PARAM_ERROR, bzf);
        return 0;
    }

    if (*bzf).writing != 0 {
        seterr!(BZ_SEQUENCE_ERROR, bzf);
        return 0;
    }

    if len == 0 {
        seterr!(BZ_OK, bzf);
        return 0;
    }

    (*bzf).strm.avail_out = len as c_uint;
    (*bzf).strm.next_out = buf as *mut c_char;

    loop {
        if sys::ferror((*bzf).handle) != 0 {
            seterr!(BZ_IO_ERROR, bzf);
            return 0;
        }

        if (*bzf).strm.avail_in == 0 && !myfeof((*bzf).handle) {
            let n = sys::fread(
                (*bzf).buf.as_mut_ptr() as *mut c_void,
                1,
                BZ_MAX_UNUSED,
                (*bzf).handle,
            );
            if sys::ferror((*bzf).handle) != 0 {
                seterr!(BZ_IO_ERROR, bzf);
                return 0;
            }
            (*bzf).buf_n = n as c_int;
            (*bzf).strm.avail_in = (*bzf).buf_n as c_uint;
            (*bzf).strm.next_in = (*bzf).buf.as_mut_ptr();
        }

        let ret = BZ2_bzDecompress(&mut (*bzf).strm);

        if ret != BZ_OK && ret != BZ_STREAM_END {
            seterr!(ret, bzf);
            return 0;
        }

        if ret == BZ_OK
            && myfeof((*bzf).handle)
            && (*bzf).strm.avail_in == 0
            && (*bzf).strm.avail_out > 0
        {
            seterr!(BZ_UNEXPECTED_EOF, bzf);
            return 0;
        }

        if ret == BZ_STREAM_END {
            seterr!(BZ_STREAM_END, bzf);
            return len - (*bzf).strm.avail_out as c_int;
        }
        if (*bzf).strm.avail_out == 0 {
            seterr!(BZ_OK, bzf);
            return len;
        }
    }
}

/*---------------------------------------------------*/
#[no_mangle]
pub unsafe extern "C" fn BZ2_bzReadGetUnused(
    bzerror: *mut c_int,
    b: *mut BZFILE,
    unused: *mut *mut c_void,
    n_unused: *mut c_int,
) {
    macro_rules! seterr {
        ($e:expr, $bzf:expr) => {{
            if !bzerror.is_null() {
                *bzerror = $e;
            }
            if !$bzf.is_null() {
                (*$bzf).last_err = $e;
            }
        }};
    }

    let bzf = b as *mut bzFile;
    if bzf.is_null() {
        seterr!(BZ_PARAM_ERROR, bzf);
        return;
    }
    if (*bzf).last_err != BZ_STREAM_END {
        seterr!(BZ_SEQUENCE_ERROR, bzf);
        return;
    }
    if unused.is_null() || n_unused.is_null() {
        seterr!(BZ_PARAM_ERROR, bzf);
        return;
    }

    seterr!(BZ_OK, bzf);
    *n_unused = (*bzf).strm.avail_in as c_int;
    *unused = (*bzf).strm.next_in as *mut c_void;
}

/*---------------------------------------------------*/
/*--- Misc convenience stuff                      ---*/
/*---------------------------------------------------*/

/*---------------------------------------------------*/
#[no_mangle]
pub unsafe extern "C" fn BZ2_bzBuffToBuffCompress(
    dest: *mut c_char,
    dest_len: *mut c_uint,
    source: *mut c_char,
    source_len: c_uint,
    block_size_100k: c_int,
    verbosity: c_int,
    work_factor: c_int,
) -> c_int {
    let mut strm: bz_stream = std::mem::zeroed();

    if dest.is_null()
        || dest_len.is_null()
        || source.is_null()
        || !(1..=9).contains(&block_size_100k)
        || !(0..=4).contains(&verbosity)
        || !(0..=250).contains(&work_factor)
    {
        return BZ_PARAM_ERROR;
    }

    let mut work_factor = work_factor;
    if work_factor == 0 {
        work_factor = 30;
    }
    strm.bzalloc = None;
    strm.bzfree = None;
    strm.opaque = std::ptr::null_mut();
    let mut ret = BZ2_bzCompressInit(&mut strm, block_size_100k, verbosity, work_factor);
    if ret != BZ_OK {
        return ret;
    }

    strm.next_in = source;
    strm.next_out = dest;
    strm.avail_in = source_len;
    strm.avail_out = *dest_len;

    ret = BZ2_bzCompress(&mut strm, BZ_FINISH);
    if ret == BZ_FINISH_OK {
        /* output_overflow: */
        BZ2_bzCompressEnd(&mut strm);
        return BZ_OUTBUFF_FULL;
    }
    if ret != BZ_STREAM_END {
        /* errhandler: */
        BZ2_bzCompressEnd(&mut strm);
        return ret;
    }

    /* normal termination */
    *dest_len -= strm.avail_out;
    BZ2_bzCompressEnd(&mut strm);
    BZ_OK
}

/*---------------------------------------------------*/
#[no_mangle]
pub unsafe extern "C" fn BZ2_bzBuffToBuffDecompress(
    dest: *mut c_char,
    dest_len: *mut c_uint,
    source: *mut c_char,
    source_len: c_uint,
    small: c_int,
    verbosity: c_int,
) -> c_int {
    let mut strm: bz_stream = std::mem::zeroed();

    if dest.is_null()
        || dest_len.is_null()
        || source.is_null()
        || (small != 0 && small != 1)
        || !(0..=4).contains(&verbosity)
    {
        return BZ_PARAM_ERROR;
    }

    strm.bzalloc = None;
    strm.bzfree = None;
    strm.opaque = std::ptr::null_mut();
    let mut ret = BZ2_bzDecompressInit(&mut strm, verbosity, small);
    if ret != BZ_OK {
        return ret;
    }

    strm.next_in = source;
    strm.next_out = dest;
    strm.avail_in = source_len;
    strm.avail_out = *dest_len;

    ret = BZ2_bzDecompress(&mut strm);
    if ret == BZ_OK {
        /* output_overflow_or_eof: */
        if strm.avail_out > 0 {
            BZ2_bzDecompressEnd(&mut strm);
            return BZ_UNEXPECTED_EOF;
        } else {
            BZ2_bzDecompressEnd(&mut strm);
            return BZ_OUTBUFF_FULL;
        };
    }
    if ret != BZ_STREAM_END {
        /* errhandler: */
        BZ2_bzDecompressEnd(&mut strm);
        return ret;
    }

    /* normal termination */
    *dest_len -= strm.avail_out;
    BZ2_bzDecompressEnd(&mut strm);
    BZ_OK
}

/*---------------------------------------------------*/
/*--
   Code contributed by Yoshioka Tsuneo (tsuneo@rr.iij4u.or.jp)
   to support better zlib compatibility.
   This code is not _officially_ part of libbzip2 (yet);
   I haven't tested it, documented it, or considered the
   threading-safeness of it.
   If this code breaks, please contact both Yoshioka and me.
--*/
/*---------------------------------------------------*/

/*---------------------------------------------------*/
/*--
   return version like "0.9.5d, 4-Sept-1999".
--*/
#[no_mangle]
pub extern "C" fn BZ2_bzlibVersion() -> *const c_char {
    BZ_VERSION.as_ptr() as *const c_char
}

/*---------------------------------------------------*/
unsafe fn bzopen_or_bzdopen(
    path: *const c_char,
    fd: c_int,
    mode: *const c_char,
    open_mode: c_int,
) -> *mut BZFILE {
    let mut bzerr = 0;
    let mut unused = [0u8; BZ_MAX_UNUSED];
    let mut block_size_100k = 9;
    let mut writing = false;
    let mut verbosity = 0;
    let work_factor = 30;
    let mut small_mode = false;
    let n_unused = 0;

    if mode.is_null() {
        return std::ptr::null_mut();
    }
    let mut p = mode;
    while *p != 0 {
        match *p as u8 {
            b'r' => writing = false,
            b'w' => writing = true,
            b's' => small_mode = true,
            c if sys::c_isdigit(c) => {
                block_size_100k = (c - BZ_HDR_0) as i32;
            }
            _ => {}
        }
        p = p.add(1);
    }
    let _ = verbosity;
    verbosity = 0;
    let mode2: &[u8] = if writing { b"wb\0" } else { b"rb\0" };

    let fp: *mut FILE = if open_mode == 0 {
        if path.is_null() || *path == 0 {
            if writing {
                sys::stdout
            } else {
                sys::stdin
            }
            /* SET_BINARY_MODE: no-op on unix */
        } else {
            sys::fopen(path, mode2.as_ptr() as *const c_char)
        }
    } else {
        sys::fdopen(fd, mode2.as_ptr() as *const c_char)
    };
    if fp.is_null() {
        return std::ptr::null_mut();
    }

    let bzfp = if writing {
        /* Guard against total chaos and anarchy -- JRS */
        if block_size_100k < 1 {
            block_size_100k = 1;
        }
        if block_size_100k > 9 {
            block_size_100k = 9;
        }
        BZ2_bzWriteOpen(
            &mut bzerr,
            fp,
            block_size_100k,
            verbosity,
            work_factor,
        )
    } else {
        BZ2_bzReadOpen(
            &mut bzerr,
            fp,
            verbosity,
            small_mode as c_int,
            unused.as_mut_ptr() as *mut c_void,
            n_unused,
        )
    };
    if bzfp.is_null() {
        if fp != sys::stdin && fp != sys::stdout {
            sys::fclose(fp);
        }
        return std::ptr::null_mut();
    }
    bzfp
}

/*---------------------------------------------------*/
/*--
   open file for read or write.
      ex) bzopen("file","w9")
      case path="" or NULL => use stdin or stdout.
--*/
#[no_mangle]
pub unsafe extern "C" fn BZ2_bzopen(path: *const c_char, mode: *const c_char) -> *mut BZFILE {
    bzopen_or_bzdopen(path, -1, mode, /*bzopen*/ 0)
}

/*---------------------------------------------------*/
#[no_mangle]
pub unsafe extern "C" fn BZ2_bzdopen(fd: c_int, mode: *const c_char) -> *mut BZFILE {
    bzopen_or_bzdopen(std::ptr::null(), fd, mode, /*bzdopen*/ 1)
}

/*---------------------------------------------------*/
#[no_mangle]
pub unsafe extern "C" fn BZ2_bzread(b: *mut BZFILE, buf: *mut c_void, len: c_int) -> c_int {
    let mut bzerr = 0;
    if (*(b as *mut bzFile)).last_err == BZ_STREAM_END {
        return 0;
    }
    let nread = BZ2_bzRead(&mut bzerr, b, buf, len);
    if bzerr == BZ_OK || bzerr == BZ_STREAM_END {
        nread
    } else {
        -1
    }
}

/*---------------------------------------------------*/
#[no_mangle]
pub unsafe extern "C" fn BZ2_bzwrite(b: *mut BZFILE, buf: *mut c_void, len: c_int) -> c_int {
    let mut bzerr = 0;

    BZ2_bzWrite(&mut bzerr, b, buf, len);
    if bzerr == BZ_OK {
        len
    } else {
        -1
    }
}

/*---------------------------------------------------*/
#[no_mangle]
pub unsafe extern "C" fn BZ2_bzflush(_b: *mut BZFILE) -> c_int {
    /* do nothing now... */
    0
}

/*---------------------------------------------------*/
#[no_mangle]
pub unsafe extern "C" fn BZ2_bzclose(b: *mut BZFILE) {
    let mut bzerr = 0;

    if b.is_null() {
        return;
    }
    let fp = (*(b as *mut bzFile)).handle;
    if (*(b as *mut bzFile)).writing != 0 {
        BZ2_bzWriteClose(
            &mut bzerr,
            b,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        if bzerr != BZ_OK {
            BZ2_bzWriteClose(
                std::ptr::null_mut(),
                b,
                1,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
        }
    } else {
        BZ2_bzReadClose(&mut bzerr, b);
    }
    if fp != sys::stdin && fp != sys::stdout {
        sys::fclose(fp);
    }
}

/*---------------------------------------------------*/
/*--
   return last error code
--*/
static BZERRORSTRINGS: [&[u8]; 16] = [
    b"OK\0",
    b"SEQUENCE_ERROR\0",
    b"PARAM_ERROR\0",
    b"MEM_ERROR\0",
    b"DATA_ERROR\0",
    b"DATA_ERROR_MAGIC\0",
    b"IO_ERROR\0",
    b"UNEXPECTED_EOF\0",
    b"OUTBUFF_FULL\0",
    b"CONFIG_ERROR\0",
    b"???\0", /* for future */
    b"???\0", /* for future */
    b"???\0", /* for future */
    b"???\0", /* for future */
    b"???\0", /* for future */
    b"???\0", /* for future */
];

#[no_mangle]
pub unsafe extern "C" fn BZ2_bzerror(b: *mut BZFILE, errnum: *mut c_int) -> *const c_char {
    let mut err = (*(b as *mut bzFile)).last_err;

    if err > 0 {
        err = 0;
    }
    *errnum = err;
    BZERRORSTRINGS[(-err) as usize].as_ptr() as *const c_char
}
