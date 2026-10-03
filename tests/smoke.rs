/*-------------------------------------------------------------*/
/*--- Smoke test: round-trip through the buffer API          ---*/
/*-------------------------------------------------------------*/

use bz2::{BZ2_bzBuffToBuffCompress, BZ2_bzBuffToBuffDecompress, BZ2_bzlibVersion};
use std::ffi::CStr;
use std::os::raw::c_uint;

fn compress(src: &[u8], block_size: i32) -> Vec<u8> {
    unsafe {
        let mut dest_len: c_uint = (src.len() + src.len() / 2 + 601) as c_uint;
        let mut dest = vec![0u8; dest_len as usize];
        let mut src_copy = src.to_vec();
        let ret = BZ2_bzBuffToBuffCompress(
            dest.as_mut_ptr() as *mut _,
            &mut dest_len,
            src_copy.as_mut_ptr() as *mut _,
            src.len() as c_uint,
            block_size,
            0,
            0,
        );
        assert_eq!(ret, 0, "compress failed");
        dest.truncate(dest_len as usize);
        dest
    }
}

fn decompress(src: &[u8], out_len: usize, small: i32) -> Vec<u8> {
    unsafe {
        let mut dest_len: c_uint = out_len as c_uint;
        let mut dest = vec![0u8; out_len.max(1)];
        let mut src_copy = src.to_vec();
        let ret = BZ2_bzBuffToBuffDecompress(
            dest.as_mut_ptr() as *mut _,
            &mut dest_len,
            src_copy.as_mut_ptr() as *mut _,
            src.len() as c_uint,
            small,
            0,
        );
        assert_eq!(ret, 0, "decompress failed: {}", ret);
        dest.truncate(dest_len as usize);
        dest
    }
}

#[test]
fn version_string() {
    unsafe {
        let v = CStr::from_ptr(BZ2_bzlibVersion());
        assert!(v.to_str().unwrap().starts_with("1.0.8"));
    }
}

#[test]
fn roundtrip_small() {
    let src = b"hello world, this is a test of bzip2 in rust. hello hello hello.";
    for bs in 1..=9 {
        let comp = compress(src, bs);
        let back = decompress(&comp, src.len(), 0);
        assert_eq!(&back[..], &src[..], "block size {}", bs);
        // small mode
        let back2 = decompress(&comp, src.len(), 1);
        assert_eq!(&back2[..], &src[..], "small mode bs {}", bs);
    }
}

#[test]
fn roundtrip_empty() {
    let src: &[u8] = b"";
    let comp = compress(src, 1);
    let back = decompress(&comp, 0, 0);
    assert_eq!(back.len(), 0);
}

#[test]
fn roundtrip_repetitive() {
    let mut src = Vec::new();
    for _ in 0..20000 {
        src.extend_from_slice(b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    }
    let comp = compress(&src, 9);
    let back = decompress(&comp, src.len(), 0);
    assert_eq!(back, src);
}

#[test]
fn roundtrip_pseudorandom() {
    /* deterministic LCG "random" data ~ incompressible */
    let mut x: u32 = 12345;
    let mut src = Vec::with_capacity(300_000);
    for _ in 0..300_000 {
        x = x.wrapping_mul(1103515245).wrapping_add(12345);
        src.push((x >> 16) as u8);
    }
    let comp = compress(&src, 9);
    let back = decompress(&comp, src.len(), 0);
    assert_eq!(back, src);
}

#[test]
fn roundtrip_multiblock() {
    /* > 900k forces multiple blocks at bs=1..3 */
    let mut x: u64 = 99991;
    let mut src = Vec::with_capacity(900_000);
    for i in 0..900_000usize {
        x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        if i % 7 == 0 {
            src.push(b'q');
        } else {
            src.push((x >> 33) as u8);
        }
    }
    for bs in [1, 2, 3] {
        let comp = compress(&src, bs);
        let back = decompress(&comp, src.len(), 0);
        assert_eq!(back, src, "multiblock bs {}", bs);
    }
}
