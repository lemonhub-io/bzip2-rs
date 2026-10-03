/*-------------------------------------------------------------*/
/*--- Huffman coding low-level stuff                        ---*/
/*---                                    huffman.c -> .rs   ---*/
/*-------------------------------------------------------------*/

/* This file is part of the Rust port of bzip2/libbzip2.
   Literal translation of huffman.c. */

use crate::bzlib::assert_h_fail;
use crate::common::*;

#[inline(always)]
fn weightof(zz0: i32) -> i32 {
    zz0 & 0xffffff00u32 as i32
}
#[inline(always)]
fn depthof(zz1: i32) -> i32 {
    zz1 & 0x000000ff
}
#[inline(always)]
fn mymax(zz2: i32, zz3: i32) -> i32 {
    if zz2 > zz3 { zz2 } else { zz3 }
}
#[inline(always)]
fn addweights(zw1: i32, zw2: i32) -> i32 {
    (weightof(zw1) + weightof(zw2)) | (1 + mymax(depthof(zw1), depthof(zw2)))
}

/*---------------------------------------------------*/
pub fn bz2_hb_make_code_lengths(len: &mut [u8], freq: &[i32], alpha_size: i32, max_len: i32) {
    /*--
       Nodes and heap entries run from 1.  Entry 0
       for both the heap and nodes is a sentinel.
    --*/
    let alpha_size = alpha_size as usize;
    let mut n_nodes: i32;
    let mut n_heap: i32;
    let mut n1: usize;
    let mut n2: usize;
    let mut too_long: bool;

    let mut heap = [0i32; BZ_MAX_ALPHA_SIZE + 2];
    let mut weight = [0i32; BZ_MAX_ALPHA_SIZE * 2];
    let mut parent = [0i32; BZ_MAX_ALPHA_SIZE * 2];

    /* UPHEAP(z): sift heap[z] up while lighter than parent */
    macro_rules! upheap {
        ($z:expr) => {{
            let mut zz = $z;
            let tmp = heap[zz];
            while weight[tmp as usize] < weight[(heap[zz >> 1]) as usize] {
                heap[zz] = heap[zz >> 1];
                zz >>= 1;
            }
            heap[zz] = tmp;
        }};
    }

    /* DOWNHEAP(z): sift heap[z] down */
    macro_rules! downheap {
        ($z:expr) => {{
            let mut zz = $z;
            let tmp = heap[zz];
            loop {
                let mut yy = zz << 1;
                if yy as i32 > n_heap {
                    break;
                }
                if (yy as i32) < n_heap
                    && weight[heap[yy + 1] as usize] < weight[heap[yy] as usize]
                {
                    yy += 1;
                }
                if weight[tmp as usize] < weight[heap[yy] as usize] {
                    break;
                }
                heap[zz] = heap[yy];
                zz = yy;
            }
            heap[zz] = tmp;
        }};
    }

    for i in 0..alpha_size {
        weight[i + 1] = (if freq[i] == 0 { 1 } else { freq[i] }) << 8;
    }

    loop {
        n_nodes = alpha_size as i32;
        n_heap = 0;

        heap[0] = 0;
        weight[0] = 0;
        parent[0] = -2;

        for i in 1..=alpha_size {
            parent[i] = -1;
            n_heap += 1;
            heap[n_heap as usize] = i as i32;
            upheap!(n_heap as usize);
        }

        assert_h(n_heap < (BZ_MAX_ALPHA_SIZE as i32 + 2), 2001);

        while n_heap > 1 {
            n1 = heap[1] as usize;
            heap[1] = heap[n_heap as usize];
            n_heap -= 1;
            downheap!(1usize);
            n2 = heap[1] as usize;
            heap[1] = heap[n_heap as usize];
            n_heap -= 1;
            downheap!(1usize);
            n_nodes += 1;
            parent[n1] = n_nodes;
            parent[n2] = n_nodes;
            weight[n_nodes as usize] = addweights(weight[n1], weight[n2]);
            parent[n_nodes as usize] = -1;
            n_heap += 1;
            heap[n_heap as usize] = n_nodes;
            upheap!(n_heap as usize);
        }

        assert_h(n_nodes < (BZ_MAX_ALPHA_SIZE as i32 * 2), 2002);

        too_long = false;
        for i in 1..=alpha_size {
            let mut j = 0;
            let mut k = i;
            while parent[k] >= 0 {
                k = parent[k] as usize;
                j += 1;
            }
            len[i - 1] = j as u8;
            if j > max_len {
                too_long = true;
            }
        }

        if !too_long {
            break;
        }

        for i in 1..=alpha_size {
            let j = weight[i] >> 8;
            let j = 1 + (j / 2);
            weight[i] = j << 8;
        }
    }
}

/*---------------------------------------------------*/
pub fn bz2_hb_assign_codes(
    code: &mut [i32],
    length: &[u8],
    min_len: i32,
    max_len: i32,
    alpha_size: i32,
) {
    let mut vec: i32 = 0;
    for n in min_len..=max_len {
        for i in 0..alpha_size as usize {
            if length[i] as i32 == n {
                code[i] = vec;
                vec += 1;
            }
        }
        vec <<= 1;
    }
}

/*---------------------------------------------------*/
pub fn bz2_hb_create_decode_tables(
    limit: &mut [i32],
    base: &mut [i32],
    perm: &mut [i32],
    length: &[u8],
    min_len: i32,
    max_len: i32,
    alpha_size: i32,
) {
    let mut pp: usize = 0;
    for i in min_len..=max_len {
        for j in 0..alpha_size as usize {
            if length[j] as i32 == i {
                perm[pp] = j as i32;
                pp += 1;
            }
        }
    }

    for i in 0..BZ_MAX_CODE_LEN {
        base[i] = 0;
    }
    for i in 0..alpha_size as usize {
        base[length[i] as usize + 1] += 1;
    }

    for i in 1..BZ_MAX_CODE_LEN {
        base[i] += base[i - 1];
    }

    for i in 0..BZ_MAX_CODE_LEN {
        limit[i] = 0;
    }
    let mut vec: i32 = 0;

    for i in min_len..=max_len {
        vec += base[(i + 1) as usize] - base[i as usize];
        limit[i as usize] = vec - 1;
        vec <<= 1;
    }
    for i in min_len + 1..=max_len {
        base[i as usize] = ((limit[(i - 1) as usize] + 1) << 1) - base[i as usize];
    }
}

/* AssertH(cond, errcode) */
#[inline(always)]
pub fn assert_h(cond: bool, errcode: i32) {
    if !cond {
        assert_h_fail(errcode);
    }
}
