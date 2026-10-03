/*-------------------------------------------------------------*/
/*--- Decompression machinery                               ---*/
/*---                                   decompress.c -> .rs ---*/
/*-------------------------------------------------------------*/

/* This file is part of the Rust port of bzip2/libbzip2.
   Literal translation of decompress.c.

   The C code implements a resumable state machine by embedding
   `case` labels of `switch (s->state)` inside the GET_BITS macros.
   This port uses a dispatch loop: each `case` label becomes a match
   arm, and suspension (RETURN(BZ_OK) on input starvation) resumes
   by re-dispatching to the arm — which re-runs the bit-accumulation
   loop with the persistent bsBuff/bsLive, exactly as in C.
   Pseudo-states (P_*) model the non-suspending control-flow
   junctions between macro expansions; they never survive a return. */

#![allow(non_snake_case)]

use crate::common::*;
use crate::bzlib::assert_h_fail;
use crate::huffman::bz2_hb_create_decode_tables;
use crate::state::{BzAlloc, DState};
use crate::sys;

/* pseudo-states internal to a single call (never visible on return) */
const P_MTF_TOP: i32 = 64;
const P_RUN_BODY: i32 = 65;
const P_AFTER_MTF3: i32 = 66;
const P_POST_MAP2: i32 = 67;
const P_POST_SEL: i32 = 68;
const P_POST_CODING: i32 = 69;
const P_POST_BLOCKS: i32 = 70;

/*---------------------------------------------------*/
fn make_maps_d(s: &mut DState) {
    s.n_in_use = 0;
    for i in 0..256 {
        if s.in_use[i] != 0 {
            s.seq_to_unseq[s.n_in_use as usize] = i as u8;
            s.n_in_use += 1;
        }
    }
}

/*---------------------------------------------------*/
/* bit-accumulation loop shared by every GET_BITS arm */
#[inline(always)]
fn gb(s: &mut DState, n: i32) -> Option<u32> {
    loop {
        if s.bs_live >= n {
            let v = (s.bs_buff >> (s.bs_live - n)) & ((1u32 << n) - 1);
            s.bs_live -= n;
            return Some(v);
        }
        let strm = s.strm;
        unsafe {
            if (*strm).avail_in == 0 {
                return None;
            }
            s.bs_buff = (s.bs_buff << 8) | (*((*strm).next_in as *const u8) as u32);
            s.bs_live += 8;
            (*strm).next_in = (*strm).next_in.add(1);
            (*strm).avail_in -= 1;
            (*strm).total_in_lo32 = (*strm).total_in_lo32.wrapping_add(1);
            if (*strm).total_in_lo32 == 0 {
                (*strm).total_in_hi32 = (*strm).total_in_hi32.wrapping_add(1);
            }
        }
    }
}

/*---------------------------------------------------*/
/* BZ2_indexIntoF from bzlib.c */
fn bz2_index_into_f(indx: i32, cftab: &[i32; 257]) -> i32 {
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
pub fn bz2_decompress(s: &mut DState) -> i32 {
    let mut uc: u8;
    let mut ret_val: i32;
    let mut min_len: i32;
    let mut max_len: i32;

    /* stuff that needs to be saved/restored */
    let mut i: i32;
    let mut j: i32;
    let mut t: i32;
    let mut alpha_size: i32;
    let mut n_groups: i32;
    let mut n_selectors: i32;
    let mut eob: i32;
    let mut group_no: i32;
    let mut group_pos: i32;
    let mut next_sym: i32;
    let mut nblock_max: i32;
    let mut nblock: i32;
    let mut es: i32;
    let mut nn: i32;
    let mut curr: i32;
    let zt: i32;
    let mut zn: i32;
    let mut zvec: i32;
    let mut zj: i32;
    let mut g_sel: i32;
    let mut g_minlen: i32;

    if s.state == BZ_X_MAGIC_1 {
        /*initialise the save area*/
        s.save_i = 0;
        s.save_j = 0;
        s.save_t = 0;
        s.save_alpha_size = 0;
        s.save_n_groups = 0;
        s.save_n_selectors = 0;
        s.save_eob = 0;
        s.save_group_no = 0;
        s.save_group_pos = 0;
        s.save_next_sym = 0;
        s.save_nblock_max = 0;
        s.save_nblock = 0;
        s.save_es = 0;
        s.save_n = 0;
        s.save_curr = 0;
        s.save_zt = 0;
        s.save_zn = 0;
        s.save_zvec = 0;
        s.save_zj = 0;
        s.save_g_sel = 0;
        s.save_g_minlen = 0;
    }

    /*restore from the save area*/
    i = s.save_i;
    j = s.save_j;
    t = s.save_t;
    alpha_size = s.save_alpha_size;
    n_groups = s.save_n_groups;
    n_selectors = s.save_n_selectors;
    eob = s.save_eob;
    group_no = s.save_group_no;
    group_pos = s.save_group_pos;
    next_sym = s.save_next_sym;
    nblock_max = s.save_nblock_max;
    nblock = s.save_nblock;
    es = s.save_es;
    nn = s.save_n;
    curr = s.save_curr;
    zt = s.save_zt;
    zn = s.save_zn;
    zvec = s.save_zvec;
    zj = s.save_zj;
    g_sel = s.save_g_sel;
    g_minlen = s.save_g_minlen;

    ret_val = BZ_OK;

    'dispatch: loop {
        match s.state {
            /*------------------------------------------------*/
            BZ_X_MAGIC_1 => {
                s.state = BZ_X_MAGIC_1;
                uc = match gb(s, 8) {
                    Some(v) => v as u8,
                    None => break 'dispatch,
                };
                if uc != BZ_HDR_B {
                    ret_val = BZ_DATA_ERROR_MAGIC;
                    break 'dispatch;
                }
                s.state = BZ_X_MAGIC_2;
            }
            BZ_X_MAGIC_2 => {
                s.state = BZ_X_MAGIC_2;
                uc = match gb(s, 8) {
                    Some(v) => v as u8,
                    None => break 'dispatch,
                };
                if uc != BZ_HDR_Z {
                    ret_val = BZ_DATA_ERROR_MAGIC;
                    break 'dispatch;
                }
                s.state = BZ_X_MAGIC_3;
            }
            BZ_X_MAGIC_3 => {
                s.state = BZ_X_MAGIC_3;
                uc = match gb(s, 8) {
                    Some(v) => v as u8,
                    None => break 'dispatch,
                };
                if uc != BZ_HDR_H {
                    ret_val = BZ_DATA_ERROR_MAGIC;
                    break 'dispatch;
                }
                s.state = BZ_X_MAGIC_4;
            }
            BZ_X_MAGIC_4 => {
                s.state = BZ_X_MAGIC_4;
                s.block_size_100k = match gb(s, 8) {
                    Some(v) => v as i32,
                    None => break 'dispatch,
                };
                if s.block_size_100k < (BZ_HDR_0 as i32 + 1)
                    || s.block_size_100k > (BZ_HDR_0 as i32 + 9)
                {
                    ret_val = BZ_DATA_ERROR_MAGIC;
                    break 'dispatch;
                }
                s.block_size_100k -= BZ_HDR_0 as i32;

                let alloc = unsafe { BzAlloc::of(s.strm) };
                if s.small_decompress != 0 {
                    let n = s.block_size_100k * 100000;
                    s.ll16 = crate::state::Arr {
                        p: alloc.alloc_arr::<u16>(n as usize, 2),
                        len: n as usize,
                    };
                    s.ll4 = crate::state::Arr {
                        p: alloc.alloc_arr::<u8>(((1 + n) >> 1) as usize, 1),
                        len: ((1 + n) >> 1) as usize,
                    };
                    if s.ll16.is_null() || s.ll4.is_null() {
                        ret_val = BZ_MEM_ERROR;
                        break 'dispatch;
                    }
                } else {
                    let n = s.block_size_100k * 100000;
                    s.tt = crate::state::Arr {
                        p: alloc.alloc_arr::<u32>(n as usize, 4),
                        len: n as usize,
                    };
                    if s.tt.is_null() {
                        ret_val = BZ_MEM_ERROR;
                        break 'dispatch;
                    }
                }
                s.state = BZ_X_BLKHDR_1;
            }

            /*------------------------------------------------*/
            BZ_X_BLKHDR_1 => {
                s.state = BZ_X_BLKHDR_1;
                uc = match gb(s, 8) {
                    Some(v) => v as u8,
                    None => break 'dispatch,
                };

                if uc == 0x17 {
                    /* goto endhdr_2 */
                    s.state = BZ_X_ENDHDR_2;
                    continue 'dispatch;
                }
                if uc != 0x31 {
                    ret_val = BZ_DATA_ERROR;
                    break 'dispatch;
                }
                s.state = BZ_X_BLKHDR_2;
            }
            BZ_X_BLKHDR_2 => {
                s.state = BZ_X_BLKHDR_2;
                uc = match gb(s, 8) {
                    Some(v) => v as u8,
                    None => break 'dispatch,
                };
                if uc != 0x41 {
                    ret_val = BZ_DATA_ERROR;
                    break 'dispatch;
                }
                s.state = BZ_X_BLKHDR_3;
            }
            BZ_X_BLKHDR_3 => {
                s.state = BZ_X_BLKHDR_3;
                uc = match gb(s, 8) {
                    Some(v) => v as u8,
                    None => break 'dispatch,
                };
                if uc != 0x59 {
                    ret_val = BZ_DATA_ERROR;
                    break 'dispatch;
                }
                s.state = BZ_X_BLKHDR_4;
            }
            BZ_X_BLKHDR_4 => {
                s.state = BZ_X_BLKHDR_4;
                uc = match gb(s, 8) {
                    Some(v) => v as u8,
                    None => break 'dispatch,
                };
                if uc != 0x26 {
                    ret_val = BZ_DATA_ERROR;
                    break 'dispatch;
                }
                s.state = BZ_X_BLKHDR_5;
            }
            BZ_X_BLKHDR_5 => {
                s.state = BZ_X_BLKHDR_5;
                uc = match gb(s, 8) {
                    Some(v) => v as u8,
                    None => break 'dispatch,
                };
                if uc != 0x53 {
                    ret_val = BZ_DATA_ERROR;
                    break 'dispatch;
                }
                s.state = BZ_X_BLKHDR_6;
            }
            BZ_X_BLKHDR_6 => {
                s.state = BZ_X_BLKHDR_6;
                uc = match gb(s, 8) {
                    Some(v) => v as u8,
                    None => break 'dispatch,
                };
                if uc != 0x59 {
                    ret_val = BZ_DATA_ERROR;
                    break 'dispatch;
                }

                s.curr_block_no += 1;
                if s.verbosity >= 2 {
                    let msg = format!("\n    [{}: huff+mtf ", s.curr_block_no);
                    unsafe {
                        sys::fputs(msg.as_ptr() as *const i8, sys::stderr);
                    }
                }

                s.stored_block_crc = 0;
                s.state = BZ_X_BCRC_1;
            }

            /*------------------------------------------------*/
            BZ_X_BCRC_1 => {
                s.state = BZ_X_BCRC_1;
                uc = match gb(s, 8) {
                    Some(v) => v as u8,
                    None => break 'dispatch,
                };
                s.stored_block_crc = (s.stored_block_crc << 8) | (uc as u32);
                s.state = BZ_X_BCRC_2;
            }
            BZ_X_BCRC_2 => {
                s.state = BZ_X_BCRC_2;
                uc = match gb(s, 8) {
                    Some(v) => v as u8,
                    None => break 'dispatch,
                };
                s.stored_block_crc = (s.stored_block_crc << 8) | (uc as u32);
                s.state = BZ_X_BCRC_3;
            }
            BZ_X_BCRC_3 => {
                s.state = BZ_X_BCRC_3;
                uc = match gb(s, 8) {
                    Some(v) => v as u8,
                    None => break 'dispatch,
                };
                s.stored_block_crc = (s.stored_block_crc << 8) | (uc as u32);
                s.state = BZ_X_BCRC_4;
            }
            BZ_X_BCRC_4 => {
                s.state = BZ_X_BCRC_4;
                uc = match gb(s, 8) {
                    Some(v) => v as u8,
                    None => break 'dispatch,
                };
                s.stored_block_crc = (s.stored_block_crc << 8) | (uc as u32);
                s.state = BZ_X_RANDBIT;
            }
            BZ_X_RANDBIT => {
                s.state = BZ_X_RANDBIT;
                s.block_randomised = match gb(s, 1) {
                    Some(v) => v as Bool,
                    None => break 'dispatch,
                };

                s.orig_ptr = 0;
                s.state = BZ_X_ORIGPTR_1;
            }
            BZ_X_ORIGPTR_1 => {
                s.state = BZ_X_ORIGPTR_1;
                uc = match gb(s, 8) {
                    Some(v) => v as u8,
                    None => break 'dispatch,
                };
                s.orig_ptr = (s.orig_ptr << 8) | (uc as i32);
                s.state = BZ_X_ORIGPTR_2;
            }
            BZ_X_ORIGPTR_2 => {
                s.state = BZ_X_ORIGPTR_2;
                uc = match gb(s, 8) {
                    Some(v) => v as u8,
                    None => break 'dispatch,
                };
                s.orig_ptr = (s.orig_ptr << 8) | (uc as i32);
                s.state = BZ_X_ORIGPTR_3;
            }
            BZ_X_ORIGPTR_3 => {
                s.state = BZ_X_ORIGPTR_3;
                uc = match gb(s, 8) {
                    Some(v) => v as u8,
                    None => break 'dispatch,
                };
                s.orig_ptr = (s.orig_ptr << 8) | (uc as i32);

                if s.orig_ptr < 0 {
                    ret_val = BZ_DATA_ERROR;
                    break 'dispatch;
                }
                if s.orig_ptr > 10 + 100000 * s.block_size_100k {
                    ret_val = BZ_DATA_ERROR;
                    break 'dispatch;
                }

                /*--- Receive the mapping table ---*/
                i = 0;
                s.state = BZ_X_MAPPING_1;
            }
            BZ_X_MAPPING_1 => {
                s.state = BZ_X_MAPPING_1;
                uc = match gb(s, 1) {
                    Some(v) => v as u8,
                    None => break 'dispatch,
                };
                if uc == 1 {
                    s.in_use16[i as usize] = 1;
                } else {
                    s.in_use16[i as usize] = 0;
                }
                i += 1;
                if i < 16 {
                    continue 'dispatch;
                }
                /* falls through in C: */
                for k in 0..256 {
                    s.in_use[k] = 0;
                }
                i = 0;
                while i < 16 && s.in_use16[i as usize] == 0 {
                    i += 1;
                }
                j = 0;
                if i < 16 {
                    s.state = BZ_X_MAPPING_2;
                    continue 'dispatch;
                }
                s.state = P_POST_MAP2;
            }
            BZ_X_MAPPING_2 => {
                s.state = BZ_X_MAPPING_2;
                uc = match gb(s, 1) {
                    Some(v) => v as u8,
                    None => break 'dispatch,
                };
                if uc == 1 {
                    s.in_use[(i * 16 + j) as usize] = 1;
                }
                j += 1;
                if j < 16 {
                    continue 'dispatch;
                }
                /* inner loop done; outer: for (i < 16 && inUse16[i]) */
                i += 1;
                while i < 16 && s.in_use16[i as usize] == 0 {
                    i += 1;
                }
                if i < 16 {
                    j = 0;
                    continue 'dispatch;
                }
                s.state = P_POST_MAP2;
            }
            P_POST_MAP2 => {
                make_maps_d(s);
                if s.n_in_use == 0 {
                    ret_val = BZ_DATA_ERROR;
                    break 'dispatch;
                }
                alpha_size = s.n_in_use + 2;

                /*--- Now the selectors ---*/
                s.state = BZ_X_SELECTOR_1;
            }
            BZ_X_SELECTOR_1 => {
                s.state = BZ_X_SELECTOR_1;
                n_groups = match gb(s, 3) {
                    Some(v) => v as i32,
                    None => break 'dispatch,
                };
                if n_groups < 2 || n_groups > BZ_N_GROUPS as i32 {
                    ret_val = BZ_DATA_ERROR;
                    break 'dispatch;
                }
                s.state = BZ_X_SELECTOR_2;
            }
            BZ_X_SELECTOR_2 => {
                s.state = BZ_X_SELECTOR_2;
                n_selectors = match gb(s, 15) {
                    Some(v) => v as i32,
                    None => break 'dispatch,
                };
                if n_selectors < 1 {
                    ret_val = BZ_DATA_ERROR;
                    break 'dispatch;
                }
                i = 0;
                j = 0;
                s.state = BZ_X_SELECTOR_3;
            }
            BZ_X_SELECTOR_3 => {
                s.state = BZ_X_SELECTOR_3;
                uc = match gb(s, 1) {
                    Some(v) => v as u8,
                    None => break 'dispatch,
                };
                if uc == 0 {
                    /* Having more than BZ_MAX_SELECTORS doesn't make much
                       sense since they will never be used, but some
                       implementations might "round up" the number of
                       selectors, so just ignore those. */
                    if i < BZ_MAX_SELECTORS as i32 {
                        s.selector_mtf[i as usize] = j as u8;
                    }
                    i += 1;
                    if i < n_selectors {
                        j = 0;
                        continue 'dispatch;
                    }
                    s.state = P_POST_SEL;
                } else {
                    j += 1;
                    if j >= n_groups {
                        ret_val = BZ_DATA_ERROR;
                        break 'dispatch;
                    }
                    continue 'dispatch;
                }
            }
            P_POST_SEL => {
                if n_selectors > BZ_MAX_SELECTORS as i32 {
                    n_selectors = BZ_MAX_SELECTORS as i32;
                }

                /*--- Undo the MTF values for the selectors. ---*/
                {
                    let mut pos = [0u8; BZ_N_GROUPS];
                    for v in 0..n_groups {
                        pos[v as usize] = v as u8;
                    }
                    for ii in 0..n_selectors as usize {
                        let mut v = s.selector_mtf[ii] as i32;
                        let tmp = pos[v as usize];
                        while v > 0 {
                            pos[v as usize] = pos[(v - 1) as usize];
                            v -= 1;
                        }
                        pos[0] = tmp;
                        s.selector[ii] = tmp;
                    }
                }

                /*--- Now the coding tables ---*/
                t = 0;
                s.state = BZ_X_CODING_1;
            }
            BZ_X_CODING_1 => {
                s.state = BZ_X_CODING_1;
                curr = match gb(s, 5) {
                    Some(v) => v as i32,
                    None => break 'dispatch,
                };
                i = 0;
                s.state = BZ_X_CODING_2;
            }
            BZ_X_CODING_2 => {
                s.state = BZ_X_CODING_2;
                if !(1..=20).contains(&curr) {
                    ret_val = BZ_DATA_ERROR;
                    break 'dispatch;
                }
                uc = match gb(s, 1) {
                    Some(v) => v as u8,
                    None => break 'dispatch,
                };
                if uc == 0 {
                    s.len[t as usize][i as usize] = curr as u8;
                    i += 1;
                    if i < alpha_size {
                        continue 'dispatch;
                    }
                    t += 1;
                    if t < n_groups {
                        s.state = BZ_X_CODING_1;
                        continue 'dispatch;
                    }
                    s.state = P_POST_CODING;
                    continue 'dispatch;
                }
                s.state = BZ_X_CODING_3;
            }
            BZ_X_CODING_3 => {
                s.state = BZ_X_CODING_3;
                uc = match gb(s, 1) {
                    Some(v) => v as u8,
                    None => break 'dispatch,
                };
                if uc == 0 {
                    curr += 1;
                } else {
                    curr -= 1;
                }
                s.state = BZ_X_CODING_2;
            }
            P_POST_CODING => {
                /*--- Create the Huffman decoding tables ---*/
                for tt in 0..n_groups as usize {
                    min_len = 32;
                    max_len = 0;
                    for ii in 0..alpha_size as usize {
                        if s.len[tt][ii] as i32 > max_len {
                            max_len = s.len[tt][ii] as i32;
                        }
                        if (s.len[tt][ii] as i32) < min_len {
                            min_len = s.len[tt][ii] as i32;
                        }
                    }
                    bz2_hb_create_decode_tables(
                        &mut s.limit[tt],
                        &mut s.base[tt],
                        &mut s.perm[tt],
                        &s.len[tt],
                        min_len,
                        max_len,
                        alpha_size,
                    );
                    s.min_lens[tt] = min_len;
                }

                /*--- Now the MTF values ---*/
                eob = s.n_in_use + 1;
                nblock_max = 100000 * s.block_size_100k;
                group_no = -1;
                group_pos = 0;

                for ii in 0..=255usize {
                    s.unzftab[ii] = 0;
                }

                /*-- MTF init --*/
                {
                    let mut kk = (MTFA_SIZE - 1) as i32;
                    for ii in (0..(256 / MTFL_SIZE) as i32).rev() {
                        for jj in (0..MTFL_SIZE as i32).rev() {
                            s.mtfa[kk as usize] = (ii * MTFL_SIZE as i32 + jj) as u8;
                            kk -= 1;
                        }
                        s.mtfbase[ii as usize] = kk + 1;
                    }
                }
                /*-- end MTF init --*/

                nblock = 0;
                /* GET_MTF_VAL(BZ_X_MTF_1, BZ_X_MTF_2, nextSym) preamble */
                if group_pos == 0 {
                    group_no += 1;
                    if group_no >= n_selectors {
                        ret_val = BZ_DATA_ERROR;
                        break 'dispatch;
                    }
                    group_pos = BZ_G_SIZE as i32;
                    g_sel = s.selector[group_no as usize] as i32;
                    g_minlen = s.min_lens[g_sel as usize];
                }
                group_pos -= 1;
                zn = g_minlen;
                s.state = BZ_X_MTF_1;
            }

            /*------------------------------------------------*/
            /* GET_MTF_VAL, call site 1: labels MTF_1 / MTF_2   */
            BZ_X_MTF_1 => {
                s.state = BZ_X_MTF_1;
                zvec = match gb(s, zn) {
                    Some(v) => v as i32,
                    None => break 'dispatch,
                };
                /* decode-test portion of the while(1) loop */
                loop {
                    if zn > 20
                    /* the longest code */
                    {
                        ret_val = BZ_DATA_ERROR;
                        break 'dispatch;
                    }
                    if zvec <= s.limit[g_sel as usize][zn as usize] {
                        break;
                    }
                    zn += 1;
                    s.state = BZ_X_MTF_2;
                    continue 'dispatch;
                }
                /* decode done */
                if zvec - s.base[g_sel as usize][zn as usize] < 0
                    || zvec - s.base[g_sel as usize][zn as usize]
                        >= BZ_MAX_ALPHA_SIZE as i32
                {
                    ret_val = BZ_DATA_ERROR;
                    break 'dispatch;
                }
                next_sym = s.perm[g_sel as usize]
                    [(zvec - s.base[g_sel as usize][zn as usize]) as usize];
                s.state = P_MTF_TOP;
            }
            BZ_X_MTF_2 => {
                s.state = BZ_X_MTF_2;
                zj = match gb(s, 1) {
                    Some(v) => v as i32,
                    None => break 'dispatch,
                };
                zvec = (zvec << 1) | zj;
                /* back to the while(1) test */
                loop {
                    if zn > 20 {
                        ret_val = BZ_DATA_ERROR;
                        break 'dispatch;
                    }
                    if zvec <= s.limit[g_sel as usize][zn as usize] {
                        break;
                    }
                    zn += 1;
                    s.state = BZ_X_MTF_2;
                    continue 'dispatch;
                }
                if zvec - s.base[g_sel as usize][zn as usize] < 0
                    || zvec - s.base[g_sel as usize][zn as usize]
                        >= BZ_MAX_ALPHA_SIZE as i32
                {
                    ret_val = BZ_DATA_ERROR;
                    break 'dispatch;
                }
                next_sym = s.perm[g_sel as usize]
                    [(zvec - s.base[g_sel as usize][zn as usize]) as usize];
                s.state = P_MTF_TOP;
            }

            /* GET_MTF_VAL, call site 2: labels MTF_3 / MTF_4   */
            BZ_X_MTF_3 => {
                s.state = BZ_X_MTF_3;
                zvec = match gb(s, zn) {
                    Some(v) => v as i32,
                    None => break 'dispatch,
                };
                loop {
                    if zn > 20 {
                        ret_val = BZ_DATA_ERROR;
                        break 'dispatch;
                    }
                    if zvec <= s.limit[g_sel as usize][zn as usize] {
                        break;
                    }
                    zn += 1;
                    s.state = BZ_X_MTF_4;
                    continue 'dispatch;
                }
                if zvec - s.base[g_sel as usize][zn as usize] < 0
                    || zvec - s.base[g_sel as usize][zn as usize]
                        >= BZ_MAX_ALPHA_SIZE as i32
                {
                    ret_val = BZ_DATA_ERROR;
                    break 'dispatch;
                }
                next_sym = s.perm[g_sel as usize]
                    [(zvec - s.base[g_sel as usize][zn as usize]) as usize];
                s.state = P_AFTER_MTF3;
            }
            BZ_X_MTF_4 => {
                s.state = BZ_X_MTF_4;
                zj = match gb(s, 1) {
                    Some(v) => v as i32,
                    None => break 'dispatch,
                };
                zvec = (zvec << 1) | zj;
                loop {
                    if zn > 20 {
                        ret_val = BZ_DATA_ERROR;
                        break 'dispatch;
                    }
                    if zvec <= s.limit[g_sel as usize][zn as usize] {
                        break;
                    }
                    zn += 1;
                    s.state = BZ_X_MTF_4;
                    continue 'dispatch;
                }
                if zvec - s.base[g_sel as usize][zn as usize] < 0
                    || zvec - s.base[g_sel as usize][zn as usize]
                        >= BZ_MAX_ALPHA_SIZE as i32
                {
                    ret_val = BZ_DATA_ERROR;
                    break 'dispatch;
                }
                next_sym = s.perm[g_sel as usize]
                    [(zvec - s.base[g_sel as usize][zn as usize]) as usize];
                s.state = P_AFTER_MTF3;
            }

            /* GET_MTF_VAL, call site 3: labels MTF_5 / MTF_6   */
            BZ_X_MTF_5 => {
                s.state = BZ_X_MTF_5;
                zvec = match gb(s, zn) {
                    Some(v) => v as i32,
                    None => break 'dispatch,
                };
                loop {
                    if zn > 20 {
                        ret_val = BZ_DATA_ERROR;
                        break 'dispatch;
                    }
                    if zvec <= s.limit[g_sel as usize][zn as usize] {
                        break;
                    }
                    zn += 1;
                    s.state = BZ_X_MTF_6;
                    continue 'dispatch;
                }
                if zvec - s.base[g_sel as usize][zn as usize] < 0
                    || zvec - s.base[g_sel as usize][zn as usize]
                        >= BZ_MAX_ALPHA_SIZE as i32
                {
                    ret_val = BZ_DATA_ERROR;
                    break 'dispatch;
                }
                next_sym = s.perm[g_sel as usize]
                    [(zvec - s.base[g_sel as usize][zn as usize]) as usize];
                s.state = P_MTF_TOP;
            }
            BZ_X_MTF_6 => {
                s.state = BZ_X_MTF_6;
                zj = match gb(s, 1) {
                    Some(v) => v as i32,
                    None => break 'dispatch,
                };
                zvec = (zvec << 1) | zj;
                loop {
                    if zn > 20 {
                        ret_val = BZ_DATA_ERROR;
                        break 'dispatch;
                    }
                    if zvec <= s.limit[g_sel as usize][zn as usize] {
                        break;
                    }
                    zn += 1;
                    s.state = BZ_X_MTF_6;
                    continue 'dispatch;
                }
                if zvec - s.base[g_sel as usize][zn as usize] < 0
                    || zvec - s.base[g_sel as usize][zn as usize]
                        >= BZ_MAX_ALPHA_SIZE as i32
                {
                    ret_val = BZ_DATA_ERROR;
                    break 'dispatch;
                }
                next_sym = s.perm[g_sel as usize]
                    [(zvec - s.base[g_sel as usize][zn as usize]) as usize];
                s.state = P_MTF_TOP;
            }

            /*------------------------------------------------*/
            /* top of `while (True)` in the MTF main loop      */
            P_MTF_TOP => {
                if next_sym == eob {
                    /* break out of the MTF loop */
                    s.state = P_POST_BLOCKS;
                    continue 'dispatch;
                }

                if next_sym == BZ_RUNA || next_sym == BZ_RUNB {
                    es = -1;
                    nn = 1;
                    s.state = P_RUN_BODY;
                    continue 'dispatch;
                } else {
                    if nblock >= nblock_max {
                        ret_val = BZ_DATA_ERROR;
                        break 'dispatch;
                    }

                    /*-- uc = MTF ( nextSym-1 ) --*/
                    {
                        let nnv = (next_sym - 1) as u32;
                        if nnv < MTFL_SIZE as u32 {
                            /* avoid general-case expense */
                            let mut nnloc = nnv as i32;
                            let pp = s.mtfbase[0] as usize;
                            uc = s.mtfa[pp + nnloc as usize];
                            while nnloc > 3 {
                                let z = pp + nnloc as usize;
                                s.mtfa[z] = s.mtfa[z - 1];
                                s.mtfa[z - 1] = s.mtfa[z - 2];
                                s.mtfa[z - 2] = s.mtfa[z - 3];
                                s.mtfa[z - 3] = s.mtfa[z - 4];
                                nnloc -= 4;
                            }
                            while nnloc > 0 {
                                s.mtfa[pp + nnloc as usize] =
                                    s.mtfa[pp + nnloc as usize - 1];
                                nnloc -= 1;
                            }
                            s.mtfa[pp] = uc;
                        } else {
                            /* general case */
                            let mut lno = (nnv as i32) / MTFL_SIZE as i32;
                            let off = (nnv as i32) % MTFL_SIZE as i32;
                            let mut pp = s.mtfbase[lno as usize] + off;
                            uc = s.mtfa[pp as usize];
                            while pp > s.mtfbase[lno as usize] {
                                s.mtfa[pp as usize] = s.mtfa[(pp - 1) as usize];
                                pp -= 1;
                            }
                            s.mtfbase[lno as usize] += 1;
                            while lno > 0 {
                                s.mtfbase[lno as usize] -= 1;
                                s.mtfa[s.mtfbase[lno as usize] as usize] = s.mtfa
                                    [(s.mtfbase[(lno - 1) as usize]
                                        + MTFL_SIZE as i32
                                        - 1)
                                        as usize];
                                lno -= 1;
                            }
                            s.mtfbase[0] -= 1;
                            s.mtfa[s.mtfbase[0] as usize] = uc;
                            if s.mtfbase[0] == 0 {
                                let mut kk = (MTFA_SIZE - 1) as i32;
                                for ii in (0..(256 / MTFL_SIZE) as i32).rev() {
                                    for jj in (0..MTFL_SIZE as i32).rev() {
                                        s.mtfa[kk as usize] = s.mtfa
                                            [(s.mtfbase[ii as usize] + jj)
                                                as usize];
                                        kk -= 1;
                                    }
                                    s.mtfbase[ii as usize] = kk + 1;
                                }
                            }
                        }
                    }
                    /*-- end uc = MTF ( nextSym-1 ) --*/

                    s.unzftab[s.seq_to_unseq[uc as usize] as usize] += 1;
                    if s.small_decompress != 0 {
                        s.ll16.set(
                            nblock as usize,
                            s.seq_to_unseq[uc as usize] as u16,
                        );
                    } else {
                        s.tt.set(
                            nblock as usize,
                            s.seq_to_unseq[uc as usize] as u32,
                        );
                    }
                    nblock += 1;

                    /* preamble of GET_MTF_VAL(MTF_5, MTF_6, nextSym) */
                    if group_pos == 0 {
                        group_no += 1;
                        if group_no >= n_selectors {
                            ret_val = BZ_DATA_ERROR;
                            break 'dispatch;
                        }
                        group_pos = BZ_G_SIZE as i32;
                        g_sel = s.selector[group_no as usize] as i32;
                        g_minlen = s.min_lens[g_sel as usize];
                    }
                    group_pos -= 1;
                    zn = g_minlen;
                    s.state = BZ_X_MTF_5;
                    continue 'dispatch;
                }
            }
            /* body of the do-while run-length loop */
            P_RUN_BODY => {
                /* Check that N doesn't get too big, so that es doesn't
                   go negative.  The maximum value that can be
                   RUNA/RUNB encoded is equal to the block size (post
                   the initial RLE), viz, 900k, so bounding N at 2
                   million should guard against overflow without
                   rejecting any legitimate inputs. */
                if nn >= 2 * 1024 * 1024 {
                    ret_val = BZ_DATA_ERROR;
                    break 'dispatch;
                }
                if next_sym == BZ_RUNA {
                    es += nn;
                } else if next_sym == BZ_RUNB {
                    es += 2 * nn;
                }
                nn *= 2;
                /* preamble of GET_MTF_VAL(MTF_3, MTF_4, nextSym) */
                if group_pos == 0 {
                    group_no += 1;
                    if group_no >= n_selectors {
                        ret_val = BZ_DATA_ERROR;
                        break 'dispatch;
                    }
                    group_pos = BZ_G_SIZE as i32;
                    g_sel = s.selector[group_no as usize] as i32;
                    g_minlen = s.min_lens[g_sel as usize];
                }
                group_pos -= 1;
                zn = g_minlen;
                s.state = BZ_X_MTF_3;
            }
            /* run-loop condition: while (nextSym == RUNA || RUNB) */
            P_AFTER_MTF3 => {
                if next_sym == BZ_RUNA || next_sym == BZ_RUNB {
                    s.state = P_RUN_BODY;
                    continue 'dispatch;
                }
                es += 1;
                uc = s.seq_to_unseq[s.mtfa[s.mtfbase[0] as usize] as usize];
                s.unzftab[uc as usize] += es;

                if s.small_decompress != 0 {
                    while es > 0 {
                        if nblock >= nblock_max {
                            ret_val = BZ_DATA_ERROR;
                            break 'dispatch;
                        }
                        s.ll16.set(nblock as usize, uc as u16);
                        nblock += 1;
                        es -= 1;
                    }
                } else {
                    while es > 0 {
                        if nblock >= nblock_max {
                            ret_val = BZ_DATA_ERROR;
                            break 'dispatch;
                        }
                        s.tt.set(nblock as usize, uc as u32);
                        nblock += 1;
                        es -= 1;
                    }
                }
                s.state = P_MTF_TOP;
            }

            /*------------------------------------------------*/
            /* after EOB: sanity checks, cftab, T^-1 build      */
            P_POST_BLOCKS => {
                /* Now we know what nblock is, we can do a better sanity
                   check on s->origPtr.
                */
                if s.orig_ptr < 0 || s.orig_ptr >= nblock {
                    ret_val = BZ_DATA_ERROR;
                    break 'dispatch;
                }

                /*-- Set up cftab to facilitate generation of T^(-1) --*/
                /* Check: unzftab entries in range. */
                for ii in 0..=255usize {
                    if s.unzftab[ii] < 0 || s.unzftab[ii] > nblock {
                        ret_val = BZ_DATA_ERROR;
                        break 'dispatch;
                    }
                }
                /* Actually generate cftab. */
                s.cftab[0] = 0;
                for ii in 1..=256usize {
                    s.cftab[ii] = s.unzftab[ii - 1];
                }
                for ii in 1..=256usize {
                    s.cftab[ii] += s.cftab[ii - 1];
                }
                /* Check: cftab entries in range. */
                for ii in 0..=256usize {
                    if s.cftab[ii] < 0 || s.cftab[ii] > nblock {
                        /* s->cftab[i] can legitimately be == nblock */
                        ret_val = BZ_DATA_ERROR;
                        break 'dispatch;
                    }
                }
                /* Check: cftab entries non-descending. */
                for ii in 1..=256usize {
                    if s.cftab[ii - 1] > s.cftab[ii] {
                        ret_val = BZ_DATA_ERROR;
                        break 'dispatch;
                    }
                }

                s.state_out_len = 0;
                s.state_out_ch = 0;
                crate::common::bz_initialise_crc(&mut s.calculated_block_crc);
                s.state = BZ_X_OUTPUT;
                if s.verbosity >= 2 {
                    unsafe {
                        sys::fputs(b"rt+rld\0".as_ptr() as *const i8, sys::stderr);
                    }
                }

                if s.small_decompress != 0 {
                    /*-- Make a copy of cftab, used in generation of T --*/
                    for ii in 0..=256usize {
                        s.cftab_copy[ii] = s.cftab[ii];
                    }

                    /*-- compute the T vector --*/
                    for ii in 0..nblock as usize {
                        let ucv = s.ll16.at(ii) as u8;
                        let cf = s.cftab_copy[ucv as usize];
                        s.set_ll(ii, cf as u32);
                        s.cftab_copy[ucv as usize] += 1;
                    }

                    /*-- Compute T^(-1) by pointer reversal on T --*/
                    let mut ii = s.orig_ptr;
                    let mut jj = s.get_ll(ii as usize) as i32;
                    loop {
                        let tmp = s.get_ll(jj as usize) as i32;
                        s.set_ll(jj as usize, ii as u32);
                        ii = jj;
                        jj = tmp;
                        if ii == s.orig_ptr {
                            break;
                        }
                    }

                    s.t_pos = s.orig_ptr as u32;
                    s.nblock_used = 0;
                    if s.block_randomised != 0 {
                        s.r_n_to_go = 0;
                        s.r_t_pos = 0;
                        /* BZ_GET_SMALL(k0) */
                        if s.t_pos >= 100000u32.wrapping_mul(s.block_size_100k as u32) {
                            ret_val = BZ_DATA_ERROR;
                            break 'dispatch;
                        }
                        s.k0 = bz2_index_into_f(s.t_pos as i32, &s.cftab);
                        s.t_pos = s.get_ll(s.t_pos as usize);
                        s.nblock_used += 1;
                        /* BZ_RAND_UPD_MASK + mask */
                        if s.r_n_to_go == 0 {
                            s.r_n_to_go =
                                crate::tables::BZ2_RNUMS[s.r_t_pos as usize] as i32;
                            s.r_t_pos += 1;
                            if s.r_t_pos == 512 {
                                s.r_t_pos = 0;
                            }
                        }
                        s.r_n_to_go -= 1;
                        s.k0 ^= if s.r_n_to_go == 1 { 1 } else { 0 };
                    } else {
                        if s.t_pos >= 100000u32.wrapping_mul(s.block_size_100k as u32) {
                            ret_val = BZ_DATA_ERROR;
                            break 'dispatch;
                        }
                        s.k0 = bz2_index_into_f(s.t_pos as i32, &s.cftab);
                        s.t_pos = s.get_ll(s.t_pos as usize);
                        s.nblock_used += 1;
                    }
                } else {
                    /*-- compute the T^(-1) vector --*/
                    for ii in 0..nblock as usize {
                        let ucv = (s.tt.at(ii) & 0xff) as u8;
                        let cf = s.cftab[ucv as usize] as usize;
                        let cur = s.tt.at(cf);
                        s.tt.set(cf, cur | ((ii as u32) << 8));
                        s.cftab[ucv as usize] += 1;
                    }

                    s.t_pos = s.tt.at(s.orig_ptr as usize) >> 8;
                    s.nblock_used = 0;
                    if s.block_randomised != 0 {
                        s.r_n_to_go = 0;
                        s.r_t_pos = 0;
                        /* BZ_GET_FAST(k0) */
                        if s.t_pos >= 100000u32.wrapping_mul(s.block_size_100k as u32) {
                            ret_val = BZ_DATA_ERROR;
                            break 'dispatch;
                        }
                        s.t_pos = s.tt.at(s.t_pos as usize);
                        s.k0 = (s.t_pos & 0xff) as i32;
                        s.t_pos >>= 8;
                        s.nblock_used += 1;
                        /* BZ_RAND_UPD_MASK + mask */
                        if s.r_n_to_go == 0 {
                            s.r_n_to_go =
                                crate::tables::BZ2_RNUMS[s.r_t_pos as usize] as i32;
                            s.r_t_pos += 1;
                            if s.r_t_pos == 512 {
                                s.r_t_pos = 0;
                            }
                        }
                        s.r_n_to_go -= 1;
                        s.k0 ^= if s.r_n_to_go == 1 { 1 } else { 0 };
                    } else {
                        if s.t_pos >= 100000u32.wrapping_mul(s.block_size_100k as u32) {
                            ret_val = BZ_DATA_ERROR;
                            break 'dispatch;
                        }
                        s.t_pos = s.tt.at(s.t_pos as usize);
                        s.k0 = (s.t_pos & 0xff) as i32;
                        s.t_pos >>= 8;
                        s.nblock_used += 1;
                    }
                }

                ret_val = BZ_OK;
                break 'dispatch;
            }

            /*------------------------------------------------*/
            BZ_X_ENDHDR_2 => {
                s.state = BZ_X_ENDHDR_2;
                uc = match gb(s, 8) {
                    Some(v) => v as u8,
                    None => break 'dispatch,
                };
                if uc != 0x72 {
                    ret_val = BZ_DATA_ERROR;
                    break 'dispatch;
                }
                s.state = BZ_X_ENDHDR_3;
            }
            BZ_X_ENDHDR_3 => {
                s.state = BZ_X_ENDHDR_3;
                uc = match gb(s, 8) {
                    Some(v) => v as u8,
                    None => break 'dispatch,
                };
                if uc != 0x45 {
                    ret_val = BZ_DATA_ERROR;
                    break 'dispatch;
                }
                s.state = BZ_X_ENDHDR_4;
            }
            BZ_X_ENDHDR_4 => {
                s.state = BZ_X_ENDHDR_4;
                uc = match gb(s, 8) {
                    Some(v) => v as u8,
                    None => break 'dispatch,
                };
                if uc != 0x38 {
                    ret_val = BZ_DATA_ERROR;
                    break 'dispatch;
                }
                s.state = BZ_X_ENDHDR_5;
            }
            BZ_X_ENDHDR_5 => {
                s.state = BZ_X_ENDHDR_5;
                uc = match gb(s, 8) {
                    Some(v) => v as u8,
                    None => break 'dispatch,
                };
                if uc != 0x50 {
                    ret_val = BZ_DATA_ERROR;
                    break 'dispatch;
                }
                s.state = BZ_X_ENDHDR_6;
            }
            BZ_X_ENDHDR_6 => {
                s.state = BZ_X_ENDHDR_6;
                uc = match gb(s, 8) {
                    Some(v) => v as u8,
                    None => break 'dispatch,
                };
                if uc != 0x90 {
                    ret_val = BZ_DATA_ERROR;
                    break 'dispatch;
                }

                s.stored_combined_crc = 0;
                s.state = BZ_X_CCRC_1;
            }
            BZ_X_CCRC_1 => {
                s.state = BZ_X_CCRC_1;
                uc = match gb(s, 8) {
                    Some(v) => v as u8,
                    None => break 'dispatch,
                };
                s.stored_combined_crc = (s.stored_combined_crc << 8) | (uc as u32);
                s.state = BZ_X_CCRC_2;
            }
            BZ_X_CCRC_2 => {
                s.state = BZ_X_CCRC_2;
                uc = match gb(s, 8) {
                    Some(v) => v as u8,
                    None => break 'dispatch,
                };
                s.stored_combined_crc = (s.stored_combined_crc << 8) | (uc as u32);
                s.state = BZ_X_CCRC_3;
            }
            BZ_X_CCRC_3 => {
                s.state = BZ_X_CCRC_3;
                uc = match gb(s, 8) {
                    Some(v) => v as u8,
                    None => break 'dispatch,
                };
                s.stored_combined_crc = (s.stored_combined_crc << 8) | (uc as u32);
                s.state = BZ_X_CCRC_4;
            }
            BZ_X_CCRC_4 => {
                s.state = BZ_X_CCRC_4;
                uc = match gb(s, 8) {
                    Some(v) => v as u8,
                    None => break 'dispatch,
                };
                s.stored_combined_crc = (s.stored_combined_crc << 8) | (uc as u32);

                s.state = BZ_X_IDLE;
                ret_val = BZ_STREAM_END;
                break 'dispatch;
            }

            /*------------------------------------------------*/
            _ => {
                assert_h_fail(4001);
            }
        }
        /* end of match */
        /* reaching here = fall-through to the next arm's state */
        continue 'dispatch;
    }

    /* save_state_and_return: */
    s.save_i = i;
    s.save_j = j;
    s.save_t = t;
    s.save_alpha_size = alpha_size;
    s.save_n_groups = n_groups;
    s.save_n_selectors = n_selectors;
    s.save_eob = eob;
    s.save_group_no = group_no;
    s.save_group_pos = group_pos;
    s.save_next_sym = next_sym;
    s.save_nblock_max = nblock_max;
    s.save_nblock = nblock;
    s.save_es = es;
    s.save_n = nn;
    s.save_curr = curr;
    s.save_zt = zt;
    s.save_zn = zn;
    s.save_zvec = zvec;
    s.save_zj = zj;
    s.save_g_sel = g_sel;
    s.save_g_minlen = g_minlen;

    ret_val
}
