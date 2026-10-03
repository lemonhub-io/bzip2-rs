/*-------------------------------------------------------------*/
/*--- Compression machinery (not incl block sorting)        ---*/
/*---                                     compress.c -> .rs ---*/
/*-------------------------------------------------------------*/

/* This file is part of the Rust port of bzip2/libbzip2.
   Literal translation of compress.c. */

#![allow(non_snake_case)]

use crate::blocksort::bz2_block_sort;
use crate::common::*;
use crate::huffman::{assert_h, bz2_hb_assign_codes, bz2_hb_make_code_lengths};
use crate::state::EState;
use crate::sys;

/*---------------------------------------------------*/
/*--- Bit stream I/O                              ---*/
/*---------------------------------------------------*/

/*---------------------------------------------------*/
pub fn bz2_bs_init_write(s: &mut EState) {
    s.bs_live = 0;
    s.bs_buff = 0;
}

/*---------------------------------------------------*/
fn bs_finish_write(s: &mut EState) {
    while s.bs_live > 0 {
        s.zbit_set(s.num_z as usize, (s.bs_buff >> 24) as u8);
        s.num_z += 1;
        s.bs_buff <<= 8;
        s.bs_live -= 8;
    }
}

/*---------------------------------------------------*/
/* bsNEEDW(nz): drain full bytes from bsBuff into zbits */
#[inline(always)]
fn bs_need_w(s: &mut EState) {
    while s.bs_live >= 8 {
        s.zbit_set(s.num_z as usize, (s.bs_buff >> 24) as u8);
        s.num_z += 1;
        s.bs_buff <<= 8;
        s.bs_live -= 8;
    }
}

/*---------------------------------------------------*/
#[inline(always)]
fn bs_w(s: &mut EState, n: i32, v: u32) {
    bs_need_w(s);
    s.bs_buff |= v << (32 - s.bs_live - n);
    s.bs_live += n;
}

/*---------------------------------------------------*/
fn bs_put_u32(s: &mut EState, u: u32) {
    bs_w(s, 8, (u >> 24) & 0xff);
    bs_w(s, 8, (u >> 16) & 0xff);
    bs_w(s, 8, (u >> 8) & 0xff);
    bs_w(s, 8, u & 0xff);
}

/*---------------------------------------------------*/
fn bs_put_uchar(s: &mut EState, c: u8) {
    bs_w(s, 8, c as u32);
}

/*---------------------------------------------------*/
/*--- The back end proper                         ---*/
/*---------------------------------------------------*/

/*---------------------------------------------------*/
fn make_maps_e(s: &mut EState) {
    s.n_in_use = 0;
    for i in 0..256 {
        if s.in_use[i] != 0 {
            s.unseq_to_seq[i] = s.n_in_use as u8;
            s.n_in_use += 1;
        }
    }
}

/*---------------------------------------------------*/
fn generate_mtf_values(s: &mut EState) {
    /*
       After sorting (eg, here),
          s->arr1 [ 0 .. s->nblock-1 ] holds sorted order,
          and
          ((UChar*)s->arr2) [ 0 .. s->nblock-1 ]
          holds the original block data.

       The first thing to do is generate the MTF values,
       and put them in
          ((UInt16*)s->arr1) [ 0 .. s->nblock-1 ].
       Because there are strictly fewer or equal MTF values
       than block values, ptr values in this area are overwritten
       with MTF values only when they are no longer needed.

       The final compressed bitstream is generated into the
       area starting at
          (UChar*) (&((UChar*)s->arr2)[s->nblock])

       These storage aliases are set up in bzCompressInit(),
       except for the last one, which is arranged in
       compressBlock().
    */
    let mut yy = [0u8; 256];
    let mut wr: i32 = 0;
    let mut z_pend: i32 = 0;

    make_maps_e(s);
    let eob = s.n_in_use + 1;

    for i in 0..=(eob as usize) {
        s.mtf_freq[i] = 0;
    }

    for i in 0..s.n_in_use as usize {
        yy[i] = i as u8;
    }

    for i in 0..s.nblock as usize {
        debug_assert!(wr as usize <= i, "generateMTFValues(1)");
        let mut j = s.arr1.at(i) as i32 - 1;
        if j < 0 {
            j += s.nblock;
        }
        let ll_i = s.unseq_to_seq[s.block_get(j as usize) as usize];
        debug_assert!((ll_i as i32) < s.n_in_use, "generateMTFValues(2a)");

        if yy[0] == ll_i {
            z_pend += 1;
        } else {
            if z_pend > 0 {
                z_pend -= 1;
                loop {
                    if z_pend & 1 != 0 {
                        s.mtfv_set(wr as usize, BZ_RUNB as u16);
                        wr += 1;
                        s.mtf_freq[BZ_RUNB as usize] += 1;
                    } else {
                        s.mtfv_set(wr as usize, BZ_RUNA as u16);
                        wr += 1;
                        s.mtf_freq[BZ_RUNA as usize] += 1;
                    }
                    if z_pend < 2 {
                        break;
                    }
                    z_pend = (z_pend - 2) / 2;
                }
                z_pend = 0;
            }
            {
                let mut rtmp = yy[1];
                yy[1] = yy[0];
                let mut ryy_j = 1usize;
                let rll_i = ll_i;
                while rll_i != rtmp {
                    ryy_j += 1;
                    let rtmp2 = rtmp;
                    rtmp = yy[ryy_j];
                    yy[ryy_j] = rtmp2;
                }
                yy[0] = rtmp;
                let j = ryy_j;
                s.mtfv_set(wr as usize, (j + 1) as u16);
                wr += 1;
                s.mtf_freq[j + 1] += 1;
            }
        }
    }

    if z_pend > 0 {
        z_pend -= 1;
        loop {
            if z_pend & 1 != 0 {
                s.mtfv_set(wr as usize, BZ_RUNB as u16);
                wr += 1;
                s.mtf_freq[BZ_RUNB as usize] += 1;
            } else {
                s.mtfv_set(wr as usize, BZ_RUNA as u16);
                wr += 1;
                s.mtf_freq[BZ_RUNA as usize] += 1;
            }
            if z_pend < 2 {
                break;
            }
            z_pend = (z_pend - 2) / 2;
        }
    }

    s.mtfv_set(wr as usize, eob as u16);
    wr += 1;
    s.mtf_freq[eob as usize] += 1;

    s.n_mtf = wr;
}

/*---------------------------------------------------*/
const BZ_LESSER_ICOST: u8 = 0;
const BZ_GREATER_ICOST: u8 = 15;

fn send_mtf_values(s: &mut EState) {
    let mut n_selectors: i32 = 0;
    let alpha_size: i32;
    let mut min_len: i32;
    let mut max_len: i32;
    let mut sel_ctr: i32;
    let n_groups: i32;
    let mut n_bytes: i32;

    let mut cost = [0u16; BZ_N_GROUPS];
    let mut fave = [0i32; BZ_N_GROUPS];

    if s.verbosity >= 3 {
        let msg = format!(
            "      {} in block, {} after MTF & 1-2 coding, {}+2 syms in use\n",
            s.nblock, s.n_mtf, s.n_in_use
        );
        unsafe {
            sys::fputs(msg.as_ptr() as *const i8, sys::stderr);
        }
    }

    alpha_size = s.n_in_use + 2;
    for t in 0..BZ_N_GROUPS {
        for v in 0..alpha_size as usize {
            s.len[t][v] = BZ_GREATER_ICOST;
        }
    }

    /*--- Decide how many coding tables to use ---*/
    assert_h(s.n_mtf > 0, 3001);
    if s.n_mtf < 200 {
        n_groups = 2;
    } else if s.n_mtf < 600 {
        n_groups = 3;
    } else if s.n_mtf < 1200 {
        n_groups = 4;
    } else if s.n_mtf < 2400 {
        n_groups = 5;
    } else {
        n_groups = 6;
    }

    /*--- Generate an initial set of coding tables ---*/
    {
        let mut n_part = n_groups;
        let mut rem_f = s.n_mtf;
        let mut gs = 0i32;
        while n_part > 0 {
            let t_freq = rem_f / n_part;
            let mut ge = gs - 1;
            let mut a_freq = 0i32;
            while a_freq < t_freq && ge < alpha_size - 1 {
                ge += 1;
                a_freq += s.mtf_freq[ge as usize];
            }

            if ge > gs && n_part != n_groups && n_part != 1 && (n_groups - n_part) % 2 == 1 {
                a_freq -= s.mtf_freq[ge as usize];
                ge -= 1;
            }

            if s.verbosity >= 3 {
                let msg = format!(
                    "      initial group {}, [{} .. {}], has {} syms ({:4.1}%)\n",
                    n_part,
                    gs,
                    ge,
                    a_freq,
                    (100.0 * a_freq as f32) / (s.n_mtf as f32)
                );
                unsafe {
                    sys::fputs(msg.as_ptr() as *const i8, sys::stderr);
                }
            }

            for v in 0..alpha_size as usize {
                if (v as i32) >= gs && (v as i32) <= ge {
                    s.len[(n_part - 1) as usize][v] = BZ_LESSER_ICOST;
                } else {
                    s.len[(n_part - 1) as usize][v] = BZ_GREATER_ICOST;
                }
            }

            n_part -= 1;
            gs = ge + 1;
            rem_f -= a_freq;
        }
    }

    /*---
      Iterate up to BZ_N_ITERS times to improve the tables.
    ---*/
    for iter in 0..BZ_N_ITERS {
        for t in 0..n_groups as usize {
            fave[t] = 0;
        }
        for t in 0..n_groups as usize {
            for v in 0..alpha_size as usize {
                s.rfreq[t][v] = 0;
            }
        }

        /*---
          Set up an auxiliary length table which is used to fast-track
          the common case (nGroups == 6).
        ---*/
        if n_groups == 6 {
            for v in 0..alpha_size as usize {
                s.len_pack[v][0] = ((s.len[1][v] as u32) << 16) | s.len[0][v] as u32;
                s.len_pack[v][1] = ((s.len[3][v] as u32) << 16) | s.len[2][v] as u32;
                s.len_pack[v][2] = ((s.len[5][v] as u32) << 16) | s.len[4][v] as u32;
            }
        }

        n_selectors = 0;
        let mut totc = 0i32;
        let mut gs = 0i32;
        loop {
            /*--- Set group start & end marks. --*/
            if gs >= s.n_mtf {
                break;
            }
            let mut ge = gs + (BZ_G_SIZE as i32) - 1;
            if ge >= s.n_mtf {
                ge = s.n_mtf - 1;
            }

            /*--
               Calculate the cost of this group as coded
               by each of the coding tables.
            --*/
            for t in 0..n_groups as usize {
                cost[t] = 0;
            }

            if n_groups == 6 && 50 == ge - gs + 1 {
                /*--- fast track the common case ---*/
                let mut cost01: u32 = 0;
                let mut cost23: u32 = 0;
                let mut cost45: u32 = 0;
                for nn in 0..50usize {
                    let icv = s.mtfv_get((gs as usize) + nn) as usize;
                    cost01 = cost01.wrapping_add(s.len_pack[icv][0]);
                    cost23 = cost23.wrapping_add(s.len_pack[icv][1]);
                    cost45 = cost45.wrapping_add(s.len_pack[icv][2]);
                }

                cost[0] = (cost01 & 0xffff) as u16;
                cost[1] = (cost01 >> 16) as u16;
                cost[2] = (cost23 & 0xffff) as u16;
                cost[3] = (cost23 >> 16) as u16;
                cost[4] = (cost45 & 0xffff) as u16;
                cost[5] = (cost45 >> 16) as u16;
            } else {
                /*--- slow version which correctly handles all situations ---*/
                for i in gs..=ge {
                    let icv = s.mtfv_get(i as usize) as usize;
                    for t in 0..n_groups as usize {
                        cost[t] = cost[t].wrapping_add(s.len[t][icv] as u16);
                    }
                }
            }

            /*--
               Find the coding table which is best for this group,
               and record its identity in the selector table.
            --*/
            let mut bc = 999999999i32;
            let mut bt = -1i32;
            for t in 0..n_groups as usize {
                if (cost[t] as i32) < bc {
                    bc = cost[t] as i32;
                    bt = t as i32;
                }
            }
            totc += bc;
            fave[bt as usize] += 1;
            s.selector[n_selectors as usize] = bt as u8;
            n_selectors += 1;

            /*--
               Increment the symbol frequencies for the selected table.
            --*/
            if n_groups == 6 && 50 == ge - gs + 1 {
                /*--- fast track the common case ---*/
                for nn in 0..50usize {
                    let icv = s.mtfv_get((gs as usize) + nn) as usize;
                    s.rfreq[bt as usize][icv] += 1;
                }
            } else {
                /*--- slow version which correctly handles all situations ---*/
                for i in gs..=ge {
                    let icv = s.mtfv_get(i as usize) as usize;
                    s.rfreq[bt as usize][icv] += 1;
                }
            }

            gs = ge + 1;
        }
        if s.verbosity >= 3 {
            let mut msg = format!("      pass {}: size is {}, grp uses are ", iter + 1, totc / 8);
            for t in 0..n_groups as usize {
                msg.push_str(&format!("{} ", fave[t]));
            }
            msg.push('\n');
            unsafe {
                sys::fputs(msg.as_ptr() as *const i8, sys::stderr);
            }
        }

        /*--
          Recompute the tables based on the accumulated frequencies.
        --*/
        /* maxLen was changed from 20 to 17 in bzip2-1.0.3.  See
           comment in huffman.c for details. */
        for t in 0..n_groups as usize {
            bz2_hb_make_code_lengths(&mut s.len[t], &s.rfreq[t], alpha_size, 17 /*20*/);
        }
    }

    assert_h(n_groups < 8, 3002);
    assert_h(
        n_selectors < 32768 && n_selectors <= BZ_MAX_SELECTORS as i32,
        3003,
    );

    /*--- Compute MTF values for the selectors. ---*/
    {
        let mut pos = [0u8; BZ_N_GROUPS];
        for i in 0..n_groups as usize {
            pos[i] = i as u8;
        }
        for i in 0..n_selectors as usize {
            let ll_i = s.selector[i];
            let mut j = 0usize;
            let mut tmp = pos[j];
            while ll_i != tmp {
                j += 1;
                let tmp2 = tmp;
                tmp = pos[j];
                pos[j] = tmp2;
            }
            pos[0] = tmp;
            s.selector_mtf[i] = j as u8;
        }
    }

    /*--- Assign actual codes for the tables. --*/
    for t in 0..n_groups as usize {
        min_len = 32;
        max_len = 0;
        for i in 0..alpha_size as usize {
            if s.len[t][i] as i32 > max_len {
                max_len = s.len[t][i] as i32;
            }
            if (s.len[t][i] as i32) < min_len {
                min_len = s.len[t][i] as i32;
            }
        }
        assert_h(!(max_len > 17 /*20*/), 3004);
        assert_h(!(min_len < 1), 3005);
        bz2_hb_assign_codes(&mut s.code[t], &s.len[t], min_len, max_len, alpha_size);
    }

    /*--- Transmit the mapping table. ---*/
    {
        let mut in_use16 = [false; 16];
        for i in 0..16usize {
            in_use16[i] = false;
            for j in 0..16usize {
                if s.in_use[i * 16 + j] != 0 {
                    in_use16[i] = true;
                }
            }
        }

        n_bytes = s.num_z;
        for i in 0..16usize {
            if in_use16[i] {
                bs_w(s, 1, 1);
            } else {
                bs_w(s, 1, 0);
            }
        }

        for i in 0..16usize {
            if in_use16[i] {
                for j in 0..16usize {
                    if s.in_use[i * 16 + j] != 0 {
                        bs_w(s, 1, 1);
                    } else {
                        bs_w(s, 1, 0);
                    }
                }
            }
        }

        if s.verbosity >= 3 {
            let msg = format!("      bytes: mapping {}, ", s.num_z - n_bytes);
            unsafe {
                sys::fputs(msg.as_ptr() as *const i8, sys::stderr);
            }
        }
    }

    /*--- Now the selectors. ---*/
    n_bytes = s.num_z;
    bs_w(s, 3, n_groups as u32);
    bs_w(s, 15, n_selectors as u32);
    for i in 0..n_selectors as usize {
        for _j in 0..s.selector_mtf[i] {
            bs_w(s, 1, 1);
        }
        bs_w(s, 1, 0);
    }
    if s.verbosity >= 3 {
        let msg = format!("selectors {}, ", s.num_z - n_bytes);
        unsafe {
            sys::fputs(msg.as_ptr() as *const i8, sys::stderr);
        }
    }

    /*--- Now the coding tables. ---*/
    n_bytes = s.num_z;

    for t in 0..n_groups as usize {
        let mut curr = s.len[t][0] as i32;
        bs_w(s, 5, curr as u32);
        for i in 0..alpha_size as usize {
            while curr < s.len[t][i] as i32 {
                bs_w(s, 2, 2);
                curr += 1; /* 10 */
            }
            while curr > s.len[t][i] as i32 {
                bs_w(s, 2, 3);
                curr -= 1; /* 11 */
            }
            bs_w(s, 1, 0);
        }
    }

    if s.verbosity >= 3 {
        let msg = format!("code lengths {}, ", s.num_z - n_bytes);
        unsafe {
            sys::fputs(msg.as_ptr() as *const i8, sys::stderr);
        }
    }

    /*--- And finally, the block data proper ---*/
    n_bytes = s.num_z;
    sel_ctr = 0;
    let mut gs = 0i32;
    loop {
        if gs >= s.n_mtf {
            break;
        }
        let mut ge = gs + (BZ_G_SIZE as i32) - 1;
        if ge >= s.n_mtf {
            ge = s.n_mtf - 1;
        }
        assert_h((s.selector[sel_ctr as usize] as i32) < n_groups, 3006);

        if n_groups == 6 && 50 == ge - gs + 1 {
            /*--- fast track the common case ---*/
            let sel = s.selector[sel_ctr as usize] as usize;
            for nn in 0..50usize {
                let mtfv_i = s.mtfv_get((gs as usize) + nn) as usize;
                bs_w(s, s.len[sel][mtfv_i] as i32, s.code[sel][mtfv_i] as u32);
            }
        } else {
            /*--- slow version which correctly handles all situations ---*/
            for i in gs..=ge {
                let sel = s.selector[sel_ctr as usize] as usize;
                let mtfv_i = s.mtfv_get(i as usize) as usize;
                bs_w(s, s.len[sel][mtfv_i] as i32, s.code[sel][mtfv_i] as u32);
            }
        }

        gs = ge + 1;
        sel_ctr += 1;
    }
    assert_h(sel_ctr == n_selectors, 3007);

    if s.verbosity >= 3 {
        let msg = format!("codes {}\n", s.num_z - n_bytes);
        unsafe {
            sys::fputs(msg.as_ptr() as *const i8, sys::stderr);
        }
    }
}

/*---------------------------------------------------*/
pub fn bz2_compress_block(s: &mut EState, is_last_block: bool) {
    if s.nblock > 0 {
        s.block_crc = !s.block_crc;
        s.combined_crc = s.combined_crc.rotate_left(1);
        s.combined_crc ^= s.block_crc;
        if s.block_no > 1 {
            s.num_z = 0;
        }

        if s.verbosity >= 2 {
            let msg = format!(
                "    block {}: crc = 0x{:08x}, combined CRC = 0x{:08x}, size = {}\n",
                s.block_no, s.block_crc, s.combined_crc, s.nblock
            );
            unsafe {
                sys::fputs(msg.as_ptr() as *const i8, sys::stderr);
            }
        }

        bz2_block_sort(s);
    }

    /* C sets s->zbits = (UChar*)(&((UChar*)s->arr2)[s->nblock]) here;
       our zbit_set() computes that base on every access instead. */

    /*-- If this is the first block, create the stream header. --*/
    if s.block_no == 1 {
        bz2_bs_init_write(s);
        bs_put_uchar(s, BZ_HDR_B);
        bs_put_uchar(s, BZ_HDR_Z);
        bs_put_uchar(s, BZ_HDR_H);
        bs_put_uchar(s, BZ_HDR_0 + s.block_size_100k as u8);
    }

    if s.nblock > 0 {
        bs_put_uchar(s, 0x31);
        bs_put_uchar(s, 0x41);
        bs_put_uchar(s, 0x59);
        bs_put_uchar(s, 0x26);
        bs_put_uchar(s, 0x53);
        bs_put_uchar(s, 0x59);

        /*-- Now the block's CRC, so it is in a known place. --*/
        bs_put_u32(s, s.block_crc);

        /*--
           Now a single bit indicating (non-)randomisation.
           As of version 0.9.5, we use a better sorting algorithm
           which makes randomisation unnecessary.  So always set
           the randomised bit to 'no'.  Of course, the decoder
           still needs to be able to handle randomised blocks
           so as to maintain backwards compatibility with
           older versions of bzip2.
        --*/
        bs_w(s, 1, 0);

        bs_w(s, 24, s.orig_ptr as u32);
        generate_mtf_values(s);
        send_mtf_values(s);
    }

    /*-- If this is the last block, add the stream trailer. --*/
    if is_last_block {
        bs_put_uchar(s, 0x17);
        bs_put_uchar(s, 0x72);
        bs_put_uchar(s, 0x45);
        bs_put_uchar(s, 0x38);
        bs_put_uchar(s, 0x50);
        bs_put_uchar(s, 0x90);
        bs_put_u32(s, s.combined_crc);
        if s.verbosity >= 2 {
            let msg = format!("    final combined CRC = 0x{:08x}\n   ", s.combined_crc);
            unsafe {
                sys::fputs(msg.as_ptr() as *const i8, sys::stderr);
            }
        }
        bs_finish_write(s);
    }
}
