/*-------------------------------------------------------------*/
/*--- Streaming test: feed/drain through tiny buffers        ---*/
/*--- exercises the resumable state machines                 ---*/
/*-------------------------------------------------------------*/

use bz2::*;
use std::os::raw::c_uint;

fn stream_decompress(comp: &[u8], in_chunk: usize, out_chunk: usize, small: i32) -> Vec<u8> {
    unsafe {
        let mut strm: bz_stream = std::mem::zeroed();
        assert_eq!(BZ2_bzDecompressInit(&mut strm, 0, small), BZ_OK);
        let mut out = Vec::new();
        let mut in_pos = 0usize;
        let mut obuf = vec![0u8; out_chunk.max(1)];
        loop {
            if strm.avail_in == 0 && in_pos < comp.len() {
                let give = (comp.len() - in_pos).min(in_chunk.max(1));
                strm.next_in = comp[in_pos..].as_ptr() as *mut i8;
                strm.avail_in = give as c_uint;
                in_pos += give;
            }
            strm.next_out = obuf.as_mut_ptr() as *mut i8;
            strm.avail_out = obuf.len() as c_uint;
            let ret = BZ2_bzDecompress(&mut strm);
            let produced = obuf.len() - strm.avail_out as usize;
            out.extend_from_slice(&obuf[..produced]);
            if ret == BZ_STREAM_END {
                break;
            }
            assert_eq!(ret, BZ_OK, "decompress ret {}", ret);
            if strm.avail_in == 0 && in_pos >= comp.len() && produced == 0 {
                panic!("decompress stalled without input");
            }
            if out_chunk == 0 && produced == 0 {
                break;
            }
        }
        BZ2_bzDecompressEnd(&mut strm);
        out
    }
}

#[test]
fn chunked_stream_roundtrip() {
    let mut x: u64 = 42;
    let mut src = Vec::new();
    for i in 0..250_000usize {
        x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        src.push(if i % 11 == 0 { b'z' } else { (x >> 33) as u8 });
    }
    /* simple driver: compress in one shot via streaming FINISH with
       full input but tiny output buffer, then decompress streaming
       with tiny input/output buffers. */
    let mut comp = Vec::new();
    unsafe {
        let mut strm: bz_stream = std::mem::zeroed();
        assert_eq!(BZ2_bzCompressInit(&mut strm, 9, 0, 0), BZ_OK);
        strm.next_in = src.as_ptr() as *mut i8;
        strm.avail_in = src.len() as c_uint;
        let mut obuf = [0i8; 97];
        let mut ret;
        loop {
            strm.next_out = obuf.as_mut_ptr();
            strm.avail_out = obuf.len() as c_uint;
            ret = BZ2_bzCompress(&mut strm, BZ_FINISH);
            let produced = obuf.len() - strm.avail_out as usize;
            comp.extend_from_slice(&obuf[..produced].iter().map(|b| *b as u8).collect::<Vec<_>>());
            if ret == BZ_STREAM_END {
                break;
            }
            assert_eq!(ret, BZ_FINISH_OK);
        }
        BZ2_bzCompressEnd(&mut strm);
    }
    for small in [0, 1] {
        for ic in [1, 7, 4096] {
            for oc in [1, 13, 8192] {
                let out = stream_decompress(&comp, ic, oc, small);
                assert_eq!(out.len(), src.len(), "len ic={} oc={} small={}", ic, oc, small);
                assert_eq!(out, src, "data ic={} oc={} small={}", ic, oc, small);
            }
        }
    }
}
