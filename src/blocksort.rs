/*-------------------------------------------------------------*/
/*--- Block sorting machinery                               ---*/
/*---                                   blocksort.c -> .rs  ---*/
/*-------------------------------------------------------------*/

/* This file is part of the Rust port of bzip2/libbzip2.
   Literal translation of blocksort.c. */

#![allow(non_snake_case)]

use crate::common::*;
use crate::huffman::assert_h;
use crate::state::{Arr, EState};
use crate::sys;

#[inline(always)]
fn vprintf0(verb: i32, f: &[u8]) {
    if verb >= 4 {
        unsafe {
            sys::fputs(f.as_ptr() as *const i8, sys::stderr);
        }
    }
}

/*---------------------------------------------*/
/*--- Fallback O(N log(N)^2) sorting        ---*/
/*--- algorithm, for repetitive blocks      ---*/
/*---------------------------------------------*/

#[inline]
fn fallback_simple_sort(fmap: &mut Arr<u32>, eclass: &Arr<u32>, lo: i32, hi: i32) {
    if lo == hi {
        return;
    }

    if hi - lo > 3 {
        let mut i = hi - 4;
        while i >= lo {
            let tmp = fmap.at(i as usize);
            let ec_tmp = eclass.at(tmp as usize);
            let mut j = i + 4;
            while j <= hi && ec_tmp > eclass.at(fmap.at(j as usize) as usize) {
                fmap.set((j - 4) as usize, fmap.at(j as usize));
                j += 4;
            }
            fmap.set((j - 4) as usize, tmp);
            i -= 1;
        }
    }

    let mut i = hi - 1;
    while i >= lo {
        let tmp = fmap.at(i as usize);
        let ec_tmp = eclass.at(tmp as usize);
        let mut j = i + 1;
        while j <= hi && ec_tmp > eclass.at(fmap.at(j as usize) as usize) {
            fmap.set((j - 1) as usize, fmap.at(j as usize));
            j += 1;
        }
        fmap.set((j - 1) as usize, tmp);
        i -= 1;
    }
}

const FALLBACK_QSORT_SMALL_THRESH: i32 = 10;
const FALLBACK_QSORT_STACK_SIZE: usize = 100;

fn fallback_qsort3(fmap: &mut Arr<u32>, eclass: &Arr<u32>, lo_st: i32, hi_st: i32) {
    let mut un_lo: i32;
    let mut un_hi: i32;
    let mut lt_lo: i32;
    let mut gt_hi: i32;
    let mut n: i32;
    let mut m: i32;
    let mut sp: usize;
    let mut lo: i32;
    let mut hi: i32;
    let mut med: u32;
    let mut r: u32 = 0;
    let mut r3: u32;

    let mut stack_lo = [0i32; FALLBACK_QSORT_STACK_SIZE];
    let mut stack_hi = [0i32; FALLBACK_QSORT_STACK_SIZE];

    macro_rules! fpush {
        ($lz:expr, $hz:expr) => {{
            stack_lo[sp] = $lz;
            stack_hi[sp] = $hz;
            sp += 1;
        }};
    }
    macro_rules! fpop {
        ($lz:ident, $hz:ident) => {{
            sp -= 1;
            $lz = stack_lo[sp];
            $hz = stack_hi[sp];
        }};
    }
    macro_rules! fvswap {
        ($zzp1:expr, $zzp2:expr, $zzn:expr) => {{
            let mut yyp1 = $zzp1;
            let mut yyp2 = $zzp2;
            let mut yyn = $zzn;
            while yyn > 0 {
                let a = fmap.at(yyp1 as usize);
                let b = fmap.at(yyp2 as usize);
                fmap.set(yyp1 as usize, b);
                fmap.set(yyp2 as usize, a);
                yyp1 += 1;
                yyp2 += 1;
                yyn -= 1;
            }
        }};
    }

    sp = 0;
    fpush!(lo_st, hi_st);

    while sp > 0 {
        assert_h((sp as i32) < (FALLBACK_QSORT_STACK_SIZE as i32) - 1, 1004);

        fpop!(lo, hi);
        if hi - lo < FALLBACK_QSORT_SMALL_THRESH {
            fallback_simple_sort(fmap, eclass, lo, hi);
            continue;
        }

        /* Random partitioning.  Median of 3 sometimes fails to
           avoid bad cases.  Median of 9 seems to help but
           looks rather expensive.  This too seems to work but
           is cheaper.  Guidance for the magic constants
           7621 and 32768 is taken from Sedgewick's algorithms
           book, chapter 35.
        */
        r = ((r.wrapping_mul(7621)).wrapping_add(1)) % 32768;
        r3 = r % 3;
        if r3 == 0 {
            med = eclass.at(fmap.at(lo as usize) as usize);
        } else if r3 == 1 {
            med = eclass.at(fmap.at(((lo + hi) >> 1) as usize) as usize);
        } else {
            med = eclass.at(fmap.at(hi as usize) as usize);
        }

        un_lo = lo;
        lt_lo = lo;
        un_hi = hi;
        gt_hi = hi;

        loop {
            loop {
                if un_lo > un_hi {
                    break;
                }
                n = eclass.at(fmap.at(un_lo as usize) as usize) as i32 - med as i32;
                if n == 0 {
                    let a = fmap.at(un_lo as usize);
                    let b = fmap.at(lt_lo as usize);
                    fmap.set(un_lo as usize, b);
                    fmap.set(lt_lo as usize, a);
                    lt_lo += 1;
                    un_lo += 1;
                    continue;
                }
                if n > 0 {
                    break;
                }
                un_lo += 1;
            }
            loop {
                if un_lo > un_hi {
                    break;
                }
                n = eclass.at(fmap.at(un_hi as usize) as usize) as i32 - med as i32;
                if n == 0 {
                    let a = fmap.at(un_hi as usize);
                    let b = fmap.at(gt_hi as usize);
                    fmap.set(un_hi as usize, b);
                    fmap.set(gt_hi as usize, a);
                    gt_hi -= 1;
                    un_hi -= 1;
                    continue;
                }
                if n < 0 {
                    break;
                }
                un_hi -= 1;
            }
            if un_lo > un_hi {
                break;
            }
            let a = fmap.at(un_lo as usize);
            let b = fmap.at(un_hi as usize);
            fmap.set(un_lo as usize, b);
            fmap.set(un_hi as usize, a);
            un_lo += 1;
            un_hi -= 1;
        }

        if gt_hi < lt_lo {
            continue;
        }

        n = std::cmp::min(lt_lo - lo, un_lo - lt_lo);
        fvswap!(lo, un_lo - n, n);
        m = std::cmp::min(hi - gt_hi, gt_hi - un_hi);
        fvswap!(un_lo, hi - m + 1, m);

        n = lo + un_lo - lt_lo - 1;
        m = hi - (gt_hi - un_hi) + 1;

        if n - lo > hi - m {
            fpush!(lo, n);
            fpush!(m, hi);
        } else {
            fpush!(m, hi);
            fpush!(lo, n);
        }
    }
}

/*---------------------------------------------*/
/* Pre:
      nblock > 0
      eclass exists for [0 .. nblock-1]
      ((UChar*)eclass) [0 .. nblock-1] holds block
      ptr exists for [0 .. nblock-1]

   Post:
      ((UChar*)eclass) [0 .. nblock-1] holds block
      All other areas of eclass destroyed
      fmap [0 .. nblock-1] holds sorted order
      bhtab [ 0 .. 2+(nblock/32) ] destroyed
*/

#[inline(always)]
fn set_bh(bhtab: &mut Arr<u32>, zz: i32) {
    let i = (zz >> 5) as usize;
    let v = bhtab.at(i) | (1u32 << (zz & 31));
    bhtab.set(i, v);
}
#[inline(always)]
fn clear_bh(bhtab: &mut Arr<u32>, zz: i32) {
    let i = (zz >> 5) as usize;
    let v = bhtab.at(i) & !(1u32 << (zz & 31));
    bhtab.set(i, v);
}
#[inline(always)]
fn isset_bh(bhtab: &Arr<u32>, zz: i32) -> bool {
    (bhtab.at((zz >> 5) as usize) & (1u32 << (zz & 31))) != 0
}
#[inline(always)]
fn word_bh(bhtab: &Arr<u32>, zz: i32) -> u32 {
    bhtab.at((zz >> 5) as usize)
}
#[inline(always)]
fn unaligned_bh(zz: i32) -> bool {
    (zz & 0x01f) != 0
}

fn fallback_sort(fmap: &mut Arr<u32>, eclass: &mut Arr<u32>, bhtab: &mut Arr<u32>, nblock: i32, verb: i32) {
    let mut ftab = [0i32; 257];
    let mut ftab_copy = [0i32; 256];
    let mut h: i32;
    let mut j: i32;
    let mut k: i32;
    let mut l: i32;
    let mut r: i32;
    let mut cc: i32;
    let mut cc1: i32;
    let mut n_not_done: i32;
    let n_bhtab: i32;

    /*--
       Initial 1-char radix sort to generate
       initial fmap and initial BH bits.
       eclass8 is the u8 view of eclass (the C code
       aliases (UChar*)eclass).
    --*/
    vprintf0(verb, b"        bucket sorting ...\n\0");
    for i in 0..257 {
        ftab[i] = 0;
    }
    for i in 0..nblock as usize {
        ftab[eclass.get8(i) as usize] += 1;
    }
    ftab_copy[..256].copy_from_slice(&ftab[..256]);
    for i in 1..257 {
        ftab[i] += ftab[i - 1];
    }

    for i in 0..nblock as usize {
        j = eclass.get8(i) as i32;
        k = ftab[j as usize] - 1;
        ftab[j as usize] = k;
        fmap.set(k as usize, i as u32);
    }

    n_bhtab = 2 + (nblock / 32);
    for i in 0..n_bhtab as usize {
        bhtab.set(i, 0);
    }
    for i in 0..256 {
        set_bh(bhtab, ftab[i]);
    }

    /*--
       Inductively refine the buckets.  Kind-of an
       "exponential radix sort" (!), inspired by the
       Manber-Myers suffix array construction algorithm.
    --*/

    /*-- set sentinel bits for block-end detection --*/
    for i in 0..32 {
        set_bh(bhtab, nblock + 2 * i);
        clear_bh(bhtab, nblock + 2 * i + 1);
    }

    /*-- the log(N) loop --*/
    h = 1;
    loop {
        if verb >= 4 {
            let msg = format!("        depth {:6} has ", h);
            unsafe {
                sys::fputs(msg.as_ptr() as *const i8, sys::stderr);
            }
        }

        j = 0;
        for i in 0..nblock {
            if isset_bh(bhtab, i) {
                j = i;
            }
            k = fmap.at(i as usize) as i32 - h;
            if k < 0 {
                k += nblock;
            }
            eclass.set(k as usize, j as u32);
        }

        n_not_done = 0;
        r = -1;
        loop {
            /*-- find the next non-singleton bucket --*/
            k = r + 1;
            while isset_bh(bhtab, k) && unaligned_bh(k) {
                k += 1;
            }
            if isset_bh(bhtab, k) {
                while word_bh(bhtab, k) == 0xffffffff {
                    k += 32;
                }
                while isset_bh(bhtab, k) {
                    k += 1;
                }
            }
            l = k - 1;
            if l >= nblock {
                break;
            }
            while !isset_bh(bhtab, k) && unaligned_bh(k) {
                k += 1;
            }
            if !isset_bh(bhtab, k) {
                while word_bh(bhtab, k) == 0x00000000 {
                    k += 32;
                }
                while !isset_bh(bhtab, k) {
                    k += 1;
                }
            }
            r = k - 1;
            if r >= nblock {
                break;
            }

            /*-- now [l, r] bracket current bucket --*/
            if r > l {
                n_not_done += r - l + 1;
                fallback_qsort3(fmap, eclass, l, r);

                /*-- scan bucket and generate header bits-- */
                cc = -1;
                let mut i = l;
                while i <= r {
                    cc1 = eclass.at(fmap.at(i as usize) as usize) as i32;
                    if cc != cc1 {
                        set_bh(bhtab, i);
                        cc = cc1;
                    }
                    i += 1;
                }
            }
        }

        if verb >= 4 {
            let msg = format!("{:6} unresolved strings\n", n_not_done);
            unsafe {
                sys::fputs(msg.as_ptr() as *const i8, sys::stderr);
            }
        }

        h *= 2;
        if h > nblock || n_not_done == 0 {
            break;
        }
    }

    /*--
       Reconstruct the original block in
       eclass8 [0 .. nblock-1], since the
       previous phase destroyed it.
    --*/
    vprintf0(verb, b"        reconstructing block ...\n\0");
    j = 0;
    let mut i: i32 = 0;
    while i < nblock {
        while ftab_copy[j as usize] == 0 {
            j += 1;
        }
        ftab_copy[j as usize] -= 1;
        eclass.set8(fmap.at(i as usize) as usize, j as u8);
        i += 1;
    }
    assert_h(j < 256, 1005);
}

/*---------------------------------------------*/
/*--- The main, O(N^2 log(N)) sorting       ---*/
/*--- algorithm.  Faster for "normal"       ---*/
/*--- non-repetitive blocks.                ---*/
/*---------------------------------------------*/

/*--
   A 12-step unrolled comparison helper.
   `block`/`quadrant` here are byte offsets into the arr2 buffer:
   block is the u8 view and quadrant is a u16 view at `qbase`.
--*/
#[allow(clippy::too_many_arguments)]
#[inline]
fn main_gtu(
    mut i1: u32,
    mut i2: u32,
    block: &Arr<u32>,
    quadrant: &Arr<u32>,
    qbase: usize,
    nblock: u32,
    budget: &mut i32,
) -> bool {
    let mut k: i32;

    macro_rules! cblk {
        () => {{
            let c1 = block.get8(i1 as usize);
            let c2 = block.get8(i2 as usize);
            if c1 != c2 {
                return c1 > c2;
            }
            i1 += 1;
            i2 += 1;
        }};
    }
    macro_rules! cq {
        () => {{
            let c1 = block.get8(i1 as usize);
            let c2 = block.get8(i2 as usize);
            if c1 != c2 {
                return c1 > c2;
            }
            let s1 = quadrant.get16(qbase + 2 * i1 as usize);
            let s2 = quadrant.get16(qbase + 2 * i2 as usize);
            if s1 != s2 {
                return s1 > s2;
            }
            i1 += 1;
            i2 += 1;
        }};
    }

    /* 1..12 unrolled */
    cblk!();
    cblk!();
    cblk!();
    cblk!();
    cblk!();
    cblk!();
    cblk!();
    cblk!();
    cblk!();
    cblk!();
    cblk!();
    cblk!();

    k = (nblock + 8) as i32;

    loop {
        /* 1..8 unrolled with quadrant */
        cq!();
        cq!();
        cq!();
        cq!();
        cq!();
        cq!();
        cq!();
        cq!();

        if i1 >= nblock {
            i1 -= nblock;
        }
        if i2 >= nblock {
            i2 -= nblock;
        }

        k -= 8;
        *budget -= 1;
        if k < 0 {
            break;
        }
    }

    false
}

/*---------------------------------------------*/
/*--
   Knuth's increments seem to work better
   than Incerpi-Sedgewick here.  Possibly
   because the number of elems to sort is
   usually small, typically <= 20.
--*/
static INCS: [i32; 14] = [
    1, 4, 13, 40, 121, 364, 1093, 3280, 9841, 29524, 88573, 265720, 797161, 2391484,
];

#[allow(clippy::too_many_arguments)]
fn main_simple_sort(
    ptr: &mut Arr<u32>,
    block: &Arr<u32>,
    quadrant: &Arr<u32>,
    qbase: usize,
    nblock: i32,
    lo: i32,
    hi: i32,
    d: i32,
    budget: &mut i32,
) {
    let big_n = hi - lo + 1;
    if big_n < 2 {
        return;
    }

    let mut hp = 0usize;
    while hp < INCS.len() && INCS[hp] < big_n {
        hp += 1;
    }
    hp = hp.wrapping_sub(1);

    loop {
        let h = INCS[hp];

        let mut i = lo + h;
        loop {
            /*-- copy 1 --*/
            if i > hi {
                break;
            }
            let v = ptr.at(i as usize);
            let mut j = i;
            while main_gtu(
                ptr.at((j - h) as usize).wrapping_add(d as u32),
                v.wrapping_add(d as u32),
                block,
                quadrant,
                qbase,
                nblock as u32,
                budget,
            ) {
                ptr.set(j as usize, ptr.at((j - h) as usize));
                j -= h;
                if j < lo + h {
                    break;
                }
            }
            ptr.set(j as usize, v);
            i += 1;

            /*-- copy 2 --*/
            if i > hi {
                break;
            }
            let v = ptr.at(i as usize);
            let mut j = i;
            while main_gtu(
                ptr.at((j - h) as usize).wrapping_add(d as u32),
                v.wrapping_add(d as u32),
                block,
                quadrant,
                qbase,
                nblock as u32,
                budget,
            ) {
                ptr.set(j as usize, ptr.at((j - h) as usize));
                j -= h;
                if j < lo + h {
                    break;
                }
            }
            ptr.set(j as usize, v);
            i += 1;

            /*-- copy 3 --*/
            if i > hi {
                break;
            }
            let v = ptr.at(i as usize);
            let mut j = i;
            while main_gtu(
                ptr.at((j - h) as usize).wrapping_add(d as u32),
                v.wrapping_add(d as u32),
                block,
                quadrant,
                qbase,
                nblock as u32,
                budget,
            ) {
                ptr.set(j as usize, ptr.at((j - h) as usize));
                j -= h;
                if j < lo + h {
                    break;
                }
            }
            ptr.set(j as usize, v);
            i += 1;

            if *budget < 0 {
                return;
            }
        }

        if hp == 0 {
            break;
        }
        hp -= 1;
    }
}

/*---------------------------------------------*/
/*--
   The following is an implementation of
   an elegant 3-way quicksort for strings,
   described in a paper "Fast Algorithms for
   Sorting and Searching Strings", by Robert
   Sedgewick and Jon L. Bentley.
--*/

#[inline(always)]
fn mmed3(a: u8, b: u8, c: u8) -> u8 {
    let mut a = a;
    let mut b = b;
    if a > b {
        std::mem::swap(&mut a, &mut b);
    }
    if b > c {
        b = c;
        if a > b {
            b = a;
        }
    }
    b
}

const MAIN_QSORT_SMALL_THRESH: i32 = 20;
const MAIN_QSORT_DEPTH_THRESH: i32 = BZ_N_RADIX + BZ_N_QSORT;
const MAIN_QSORT_STACK_SIZE: usize = 100;

#[allow(clippy::too_many_arguments)]
fn main_qsort3(
    ptr: &mut Arr<u32>,
    block: &Arr<u32>,
    quadrant: &Arr<u32>,
    qbase: usize,
    nblock: i32,
    lo_st: i32,
    hi_st: i32,
    d_st: i32,
    budget: &mut i32,
) {
    let mut un_lo: i32;
    let mut un_hi: i32;
    let mut lt_lo: i32;
    let mut gt_hi: i32;
    let mut n: i32;
    let mut m: i32;
    let mut med: i32;
    let mut sp: usize;
    let mut lo: i32;
    let mut hi: i32;
    let mut d: i32;

    let mut stack_lo = [0i32; MAIN_QSORT_STACK_SIZE];
    let mut stack_hi = [0i32; MAIN_QSORT_STACK_SIZE];
    let mut stack_d = [0i32; MAIN_QSORT_STACK_SIZE];

    let mut next_lo = [0i32; 3];
    let mut next_hi = [0i32; 3];
    let mut next_d = [0i32; 3];

    macro_rules! mpush {
        ($lz:expr, $hz:expr, $dz:expr) => {{
            stack_lo[sp] = $lz;
            stack_hi[sp] = $hz;
            stack_d[sp] = $dz;
            sp += 1;
        }};
    }
    macro_rules! mpop {
        ($lz:ident, $hz:ident, $dz:ident) => {{
            sp -= 1;
            $lz = stack_lo[sp];
            $hz = stack_hi[sp];
            $dz = stack_d[sp];
        }};
    }
    macro_rules! mvswap {
        ($zzp1:expr, $zzp2:expr, $zzn:expr) => {{
            let mut yyp1 = $zzp1;
            let mut yyp2 = $zzp2;
            let mut yyn = $zzn;
            while yyn > 0 {
                let a = ptr.at(yyp1 as usize);
                let b = ptr.at(yyp2 as usize);
                ptr.set(yyp1 as usize, b);
                ptr.set(yyp2 as usize, a);
                yyp1 += 1;
                yyp2 += 1;
                yyn -= 1;
            }
        }};
    }

    sp = 0;
    mpush!(lo_st, hi_st, d_st);

    while sp > 0 {
        assert_h((sp as i32) < (MAIN_QSORT_STACK_SIZE as i32) - 2, 1001);

        mpop!(lo, hi, d);
        if hi - lo < MAIN_QSORT_SMALL_THRESH || d > MAIN_QSORT_DEPTH_THRESH {
            main_simple_sort(ptr, block, quadrant, qbase, nblock, lo, hi, d, budget);
            if *budget < 0 {
                return;
            }
            continue;
        }

        med = mmed3(
            block.get8((ptr.at(lo as usize).wrapping_add(d as u32)) as usize),
            block.get8((ptr.at(hi as usize).wrapping_add(d as u32)) as usize),
            block.get8((ptr.at(((lo + hi) >> 1) as usize).wrapping_add(d as u32)) as usize),
        ) as i32;

        un_lo = lo;
        lt_lo = lo;
        un_hi = hi;
        gt_hi = hi;

        loop {
            loop {
                if un_lo > un_hi {
                    break;
                }
                n = block.get8((ptr.at(un_lo as usize).wrapping_add(d as u32)) as usize) as i32
                    - med;
                if n == 0 {
                    let a = ptr.at(un_lo as usize);
                    let b = ptr.at(lt_lo as usize);
                    ptr.set(un_lo as usize, b);
                    ptr.set(lt_lo as usize, a);
                    lt_lo += 1;
                    un_lo += 1;
                    continue;
                }
                if n > 0 {
                    break;
                }
                un_lo += 1;
            }
            loop {
                if un_lo > un_hi {
                    break;
                }
                n = block.get8((ptr.at(un_hi as usize).wrapping_add(d as u32)) as usize) as i32
                    - med;
                if n == 0 {
                    let a = ptr.at(un_hi as usize);
                    let b = ptr.at(gt_hi as usize);
                    ptr.set(un_hi as usize, b);
                    ptr.set(gt_hi as usize, a);
                    gt_hi -= 1;
                    un_hi -= 1;
                    continue;
                }
                if n < 0 {
                    break;
                }
                un_hi -= 1;
            }
            if un_lo > un_hi {
                break;
            }
            let a = ptr.at(un_lo as usize);
            let b = ptr.at(un_hi as usize);
            ptr.set(un_lo as usize, b);
            ptr.set(un_hi as usize, a);
            un_lo += 1;
            un_hi -= 1;
        }

        if gt_hi < lt_lo {
            mpush!(lo, hi, d + 1);
            continue;
        }

        n = std::cmp::min(lt_lo - lo, un_lo - lt_lo);
        mvswap!(lo, un_lo - n, n);
        m = std::cmp::min(hi - gt_hi, gt_hi - un_hi);
        mvswap!(un_lo, hi - m + 1, m);

        n = lo + un_lo - lt_lo - 1;
        m = hi - (gt_hi - un_hi) + 1;

        next_lo[0] = lo;
        next_hi[0] = n;
        next_d[0] = d;
        next_lo[1] = m;
        next_hi[1] = hi;
        next_d[1] = d;
        next_lo[2] = n + 1;
        next_hi[2] = m - 1;
        next_d[2] = d + 1;

        fn mnextsize(next_lo: &[i32; 3], next_hi: &[i32; 3], az: usize) -> i32 {
            next_hi[az] - next_lo[az]
        }
        macro_rules! mnextswap {
            ($az:expr, $bz:expr) => {{
                next_lo.swap($az, $bz);
                next_hi.swap($az, $bz);
                next_d.swap($az, $bz);
            }};
        }

        if mnextsize(&next_lo, &next_hi, 0) < mnextsize(&next_lo, &next_hi, 1) {
            mnextswap!(0, 1);
        }
        if mnextsize(&next_lo, &next_hi, 1) < mnextsize(&next_lo, &next_hi, 2) {
            mnextswap!(1, 2);
        }
        if mnextsize(&next_lo, &next_hi, 0) < mnextsize(&next_lo, &next_hi, 1) {
            mnextswap!(0, 1);
        }

        mpush!(next_lo[0], next_hi[0], next_d[0]);
        mpush!(next_lo[1], next_hi[1], next_d[1]);
        mpush!(next_lo[2], next_hi[2], next_d[2]);
    }
}

/*---------------------------------------------*/
/* Pre:
      nblock > N_OVERSHOOT
      block32 exists for [0 .. nblock-1 +N_OVERSHOOT]
      ((UChar*)block32) [0 .. nblock-1] holds block
      ptr exists for [0 .. nblock-1]

   Post:
      ((UChar*)block32) [0 .. nblock-1] holds block
      All other areas of block32 destroyed
      ftab [0 .. 65536 ] destroyed
      ptr [0 .. nblock-1] holds sorted order
      if (*budget < 0), sorting was abandoned
*/

const SETMASK: u32 = 1 << 21;
const CLEARMASK: u32 = !SETMASK;

#[allow(clippy::too_many_arguments)]
fn main_sort(
    ptr: &mut Arr<u32>,
    block: &mut Arr<u32>,
    quadrant: &mut Arr<u32>,
    qbase: usize,
    ftab: &mut Arr<u32>,
    nblock: i32,
    verb: i32,
    budget: &mut i32,
) {
    let mut running_order = [0i32; 256];
    let mut big_done = [false; 256];
    let mut copy_start = [0i32; 256];
    let mut copy_end = [0i32; 256];
    let mut num_qsorted: i32 = 0;

    macro_rules! bigfreq {
        ($b:expr) => {{
            (ftab.at((($b as usize) + 1) << 8) - ftab.at(($b as usize) << 8)) as i32
        }};
    }

    if verb >= 4 {
        vprintf0(verb, b"        main sort initialise ...\n\0");
    }

    /*-- set up the 2-byte frequency table --*/
    let mut i = 65536i32;
    while i >= 0 {
        ftab.set(i as usize, 0);
        i -= 1;
    }

    let mut j: u32 = (block.get8(0) as u32) << 8;
    let mut ii = nblock - 1;
    while ii >= 3 {
        quadrant.set16(qbase + 2 * ii as usize, 0);
        j = (j >> 8) | ((block.get8(ii as usize) as u32) << 8);
        {
            let v = ftab.at(j as usize).wrapping_add(1);
            ftab.set(j as usize, v);
        }
        quadrant.set16(qbase + 2 * (ii - 1) as usize, 0);
        j = (j >> 8) | ((block.get8((ii - 1) as usize) as u32) << 8);
        {
            let v = ftab.at(j as usize).wrapping_add(1);
            ftab.set(j as usize, v);
        }
        quadrant.set16(qbase + 2 * (ii - 2) as usize, 0);
        j = (j >> 8) | ((block.get8((ii - 2) as usize) as u32) << 8);
        {
            let v = ftab.at(j as usize).wrapping_add(1);
            ftab.set(j as usize, v);
        }
        quadrant.set16(qbase + 2 * (ii - 3) as usize, 0);
        j = (j >> 8) | ((block.get8((ii - 3) as usize) as u32) << 8);
        {
            let v = ftab.at(j as usize).wrapping_add(1);
            ftab.set(j as usize, v);
        }
        ii -= 4;
    }
    while ii >= 0 {
        quadrant.set16(qbase + 2 * ii as usize, 0);
        j = (j >> 8) | ((block.get8(ii as usize) as u32) << 8);
        {
            let v = ftab.at(j as usize).wrapping_add(1);
            ftab.set(j as usize, v);
        }
        ii -= 1;
    }

    /*-- (emphasises close relationship of block & quadrant) --*/
    for i in 0..BZ_N_OVERSHOOT {
        let v = block.get8(i);
        block.set8(nblock as usize + i, v);
        quadrant.set16(qbase + 2 * (nblock as usize + i), 0);
    }

    vprintf0(verb, b"        bucket sorting ...\n\0");

    /*-- Complete the initial radix sort --*/
    for i in 1..=65536usize {
        let v = ftab.at(i).wrapping_add(ftab.at(i - 1));
        ftab.set(i, v);
    }

    let mut s: u32 = (block.get8(0) as u32) << 8;
    let mut i = nblock - 1;
    while i >= 3 {
        s = (s >> 8) | ((block.get8(i as usize) as u32) << 8);
        j = ftab.at(s as usize) - 1;
        ftab.set(s as usize, j);
        ptr.set(j as usize, i as u32);
        s = (s >> 8) | ((block.get8((i - 1) as usize) as u32) << 8);
        j = ftab.at(s as usize) - 1;
        ftab.set(s as usize, j);
        ptr.set(j as usize, (i - 1) as u32);
        s = (s >> 8) | ((block.get8((i - 2) as usize) as u32) << 8);
        j = ftab.at(s as usize) - 1;
        ftab.set(s as usize, j);
        ptr.set(j as usize, (i - 2) as u32);
        s = (s >> 8) | ((block.get8((i - 3) as usize) as u32) << 8);
        j = ftab.at(s as usize) - 1;
        ftab.set(s as usize, j);
        ptr.set(j as usize, (i - 3) as u32);
        i -= 4;
    }
    while i >= 0 {
        s = (s >> 8) | ((block.get8(i as usize) as u32) << 8);
        j = ftab.at(s as usize) - 1;
        ftab.set(s as usize, j);
        ptr.set(j as usize, i as u32);
        i -= 1;
    }

    /*--
       Now ftab contains the first loc of every small bucket.
       Calculate the running order, from smallest to largest
       big bucket.
    --*/
    for i in 0..256usize {
        big_done[i] = false;
        running_order[i] = i as i32;
    }

    {
        let mut h = 1i32;
        loop {
            h = 3 * h + 1;
            if h > 256 {
                break;
            }
        }
        loop {
            h /= 3;
            let mut i = h;
            while i <= 255 {
                let vv = running_order[i as usize];
                let mut j = i;
                while bigfreq!(running_order[(j - h) as usize]) > bigfreq!(vv) {
                    running_order[j as usize] = running_order[(j - h) as usize];
                    j -= h;
                    if j < h {
                        break;
                    }
                }
                running_order[j as usize] = vv;
                i += 1;
            }
            if h == 1 {
                break;
            }
        }
    }

    /*--
       The main sorting loop.
    --*/

    for i in 0..256usize {
        /*--
           Process big buckets, starting with the least full.
        --*/
        let ss = running_order[i];

        /*-- Step 1 --*/
        for j in 0..256i32 {
            if j != ss {
                let sb = (ss << 8) + j;
                if (ftab.at(sb as usize) & SETMASK) == 0 {
                    let lo = (ftab.at(sb as usize) & CLEARMASK) as i32;
                    let hi = ((ftab.at((sb + 1) as usize) & CLEARMASK) as i32) - 1;
                    if hi > lo {
                        if verb >= 4 {
                            let msg = format!(
                                "        qsort [0x{:x}, 0x{:x}]   done {}   this {}\n",
                                ss,
                                j,
                                num_qsorted,
                                hi - lo + 1
                            );
                            unsafe {
                                sys::fputs(msg.as_ptr() as *const i8, sys::stderr);
                            }
                        }
                        main_qsort3(
                            ptr,
                            block,
                            quadrant,
                            qbase,
                            nblock,
                            lo,
                            hi,
                            BZ_N_RADIX,
                            budget,
                        );
                        num_qsorted += hi - lo + 1;
                        if *budget < 0 {
                            return;
                        }
                    }
                }
                let v = ftab.at(sb as usize) | SETMASK;
                ftab.set(sb as usize, v);
            }
        }

        assert_h(!big_done[ss as usize], 1006);

        /*-- Step 2 --*/
        {
            for j in 0..256i32 {
                copy_start[j as usize] = (ftab.at(((j << 8) + ss) as usize) & CLEARMASK) as i32;
                copy_end[j as usize] =
                    (ftab.at(((j << 8) + ss + 1) as usize) & CLEARMASK) as i32 - 1;
            }
            let mut j = (ftab.at((ss << 8) as usize) & CLEARMASK) as i32;
            while j < copy_start[ss as usize] {
                let mut k = ptr.at(j as usize) as i32 - 1;
                if k < 0 {
                    k += nblock;
                }
                let c1 = block.get8(k as usize);
                if !big_done[c1 as usize] {
                    let p = copy_start[c1 as usize];
                    ptr.set(p as usize, k as u32);
                    copy_start[c1 as usize] += 1;
                }
                j += 1;
            }
            let mut j = ((ftab.at(((ss + 1) << 8) as usize) & CLEARMASK) as i32) - 1;
            while j > copy_end[ss as usize] {
                let mut k = ptr.at(j as usize) as i32 - 1;
                if k < 0 {
                    k += nblock;
                }
                let c1 = block.get8(k as usize);
                if !big_done[c1 as usize] {
                    let p = copy_end[c1 as usize];
                    ptr.set(p as usize, k as u32);
                    copy_end[c1 as usize] -= 1;
                }
                j -= 1;
            }
        }

        assert_h(
            (copy_start[ss as usize] - 1 == copy_end[ss as usize])
                || (copy_start[ss as usize] == 0 && copy_end[ss as usize] == nblock - 1),
            1007,
        );

        for j in 0..256i32 {
            let idx = ((j << 8) + ss) as usize;
            let v = ftab.at(idx) | SETMASK;
            ftab.set(idx, v);
        }

        /*-- Step 3 --*/
        big_done[ss as usize] = true;

        if i < 255 {
            let bb_start = (ftab.at((ss << 8) as usize) & CLEARMASK) as i32;
            let bb_size = ((ftab.at(((ss + 1) << 8) as usize) & CLEARMASK) as i32) - bb_start;
            let mut shifts = 0i32;

            while (bb_size >> shifts) > 65534 {
                shifts += 1;
            }

            let mut j = bb_size - 1;
            while j >= 0 {
                let a2update = ptr.at((bb_start + j) as usize) as usize;
                let q_val = (j >> shifts) as u16;
                quadrant.set16(qbase + 2 * a2update, q_val);
                if a2update < BZ_N_OVERSHOOT {
                    quadrant.set16(qbase + 2 * (a2update + nblock as usize), q_val);
                }
                j -= 1;
            }
            assert_h(((bb_size - 1) >> shifts) <= 65535, 1002);
        }
    }

    if verb >= 4 {
        let msg = format!(
            "        {} pointers, {} sorted, {} scanned\n",
            nblock,
            num_qsorted,
            nblock - num_qsorted
        );
        unsafe {
            sys::fputs(msg.as_ptr() as *const i8, sys::stderr);
        }
    }
}

/*---------------------------------------------*/
/* Pre:
      nblock > 0
      arr2 exists for [0 .. nblock-1 +N_OVERSHOOT]
      ((UChar*)arr2)  [0 .. nblock-1] holds block
      arr1 exists for [0 .. nblock-1]

   Post:
      ((UChar*)arr2) [0 .. nblock-1] holds block
      All other areas of block destroyed
      ftab [ 0 .. 65536 ] destroyed
      arr1 [0 .. nblock-1] holds sorted order
*/
pub fn bz2_block_sort(s: &mut EState) {
    let nblock = s.nblock;
    let verb = s.verbosity;
    let mut wfact = s.work_factor;
    let mut budget: i32;

    if nblock < 10000 {
        fallback_sort(&mut s.arr1, &mut s.arr2, &mut s.ftab, nblock, verb);
    } else {
        /* Calculate the location for quadrant, remembering to get
           the alignment right.  Assumes that &(block[0]) is at least
           2-byte aligned -- this should be ok since block is really
           the first section of arr2.  qbase is the byte offset of
           the u16 quadrant view inside arr2. */
        let mut qb = nblock as usize + BZ_N_OVERSHOOT;
        if qb & 1 != 0 {
            qb += 1;
        }

        /* (wfact-1) / 3 puts the default-factor-30
           transition point at very roughly the same place as
           with v0.1 and v0.9.0.  Not that it particularly
           matters any more, since the resulting compressed
           stream is now the same regardless of whether or
           not we use the main sort or fallback sort. */
        if wfact < 1 {
            wfact = 1;
        }
        if wfact > 100 {
            wfact = 100;
        }
        let budget_init = nblock * ((wfact - 1) / 3);
        budget = budget_init;

        let mut block_v = s.arr2;
        let mut quad_v = s.arr2;
        main_sort(
            &mut s.arr1,
            &mut block_v,
            &mut quad_v,
            qb,
            &mut s.ftab,
            nblock,
            verb,
            &mut budget,
        );
        if verb >= 3 {
            let ratio = (budget_init - budget) as f64 / (if nblock == 0 { 1 } else { nblock }) as f64;
            let msg = format!(
                "      {} work, {} block, ratio {:5.2}\n",
                budget_init - budget,
                nblock,
                ratio
            );
            unsafe {
                sys::fputs(msg.as_ptr() as *const i8, sys::stderr);
            }
        }
        if budget < 0 {
            if verb >= 2 {
                unsafe {
                    sys::fputs(
                        b"    too repetitive; using fallback sorting algorithm\n\0".as_ptr()
                            as *const i8,
                        sys::stderr,
                    );
                }
            }
            fallback_sort(&mut s.arr1, &mut s.arr2, &mut s.ftab, nblock, verb);
        }
    }

    s.orig_ptr = -1;
    for i in 0..s.nblock as usize {
        if s.arr1.at(i) == 0 {
            s.orig_ptr = i as i32;
            break;
        }
    }

    assert_h(s.orig_ptr != -1, 1003);
}
