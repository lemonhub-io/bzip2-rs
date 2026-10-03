/*-------------------------------------------------------------*/
/*--- Differential test vs the C bzip2 binary                ---*/
/*---                                                        ---*/
/*--- Compress each input with the Rust library and with the ---*/
/*--- original C `bzip2` (env BZIP2_C, default ../bzip2/     ---*/
/*--- bzip2 relative to the crate root) and require identical */
/*--- bytes.  Then decompress the C output with the Rust lib  */
/*--- and require the original input back.                    ---*/
/*-------------------------------------------------------------*/

use bz2::{BZ2_bzBuffToBuffCompress, BZ2_bzBuffToBuffDecompress};
use std::io::Write;
use std::os::raw::c_uint;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn c_bzip2() -> PathBuf {
    std::env::var("BZIP2_C")
        .map(PathBuf::from)
        .unwrap_or_else(|_| crate_root().join("../bzip2/bzip2"))
}

fn rust_compress(src: &[u8], block_size: i32) -> Vec<u8> {
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
        assert_eq!(ret, 0, "rust compress failed");
        dest.truncate(dest_len as usize);
        dest
    }
}

fn rust_decompress(src: &[u8], out_len: usize, small: i32) -> Vec<u8> {
    unsafe {
        let mut dest_len: c_uint = out_len.max(1) as c_uint;
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
        assert_eq!(ret, 0, "rust decompress failed: {}", ret);
        dest.truncate(dest_len as usize);
        dest
    }
}

fn c_compress(src: &[u8], block_size: i32) -> Vec<u8> {
    let mut child = Command::new(c_bzip2())
        .args(["-c", &format!("-{}", block_size)])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn C bzip2");
    let mut stdin = child.stdin.take().unwrap();
    let src_owned = src.to_vec();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(&src_owned);
    });
    let out = child.wait_with_output().expect("wait C bzip2");
    writer.join().unwrap();
    assert!(out.status.success(), "C bzip2 failed");
    out.stdout
}

fn check_one(name: &str, src: &[u8], block_size: i32) {
    let c_out = c_compress(src, block_size);
    let r_out = rust_compress(src, block_size);
    assert_eq!(
        r_out, c_out,
        "compress output differs for {} (bs={}): rust {} bytes vs C {} bytes",
        name,
        block_size,
        r_out.len(),
        c_out.len()
    );
    /* decompress the C output through the Rust decoder (both modes) */
    let back = rust_decompress(&c_out, src.len(), 0);
    assert_eq!(back, src, "decompress(C output) mismatch for {}", name);
    let back_s = rust_decompress(&c_out, src.len(), 1);
    assert_eq!(back_s, src, "small decompress mismatch for {}", name);
    /* decompress the Rust output through the C binary */
    let mut child = Command::new(c_bzip2())
        .args(["-dc"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn C bzip2 -d");
    let mut stdin = child.stdin.take().unwrap();
    let r_copy = r_out.clone();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(&r_copy);
    });
    let out = child.wait_with_output().expect("wait");
    writer.join().unwrap();
    assert!(out.status.success(), "C bzip2 -d rejected Rust output");
    assert_eq!(out.stdout, src, "C decompress of Rust output mismatch");
}

fn corpus() -> Vec<(String, Vec<u8>)> {
    let mut v: Vec<(String, Vec<u8>)> = Vec::new();
    v.push(("empty".into(), vec![]));
    v.push(("one".into(), b"a".to_vec()));
    v.push(("hello".into(), b"hello world\n".to_vec()));
    v.push(("run-of-a".into(), vec![b'a'; 10000]));
    v.push(("alphabet".into(), {
        let mut d = Vec::new();
        for _ in 0..500 {
            d.extend_from_slice(b"abcdefghijklmnopqrstuvwxyz");
        }
        d
    }));
    v.push(("lcg-300k".into(), {
        let mut x: u32 = 0xC0FFEE;
        (0..300_000)
            .map(|_| {
                x = x.wrapping_mul(1103515245).wrapping_add(12345);
                (x >> 16) as u8
            })
            .collect()
    }));
    v.push(("all-bytes".into(), {
        let mut d = Vec::new();
        for _ in 0..64 {
            d.extend(0u8..=255);
        }
        d
    }));
    v.push(("text-corpus".into(), {
        let mut d = Vec::new();
        let words: &[&[u8]] = &[
            b"the ", b"quick ", b"brown ", b"fox ", b"jumps ", b"over ", b"lazy ", b"dog. ",
            b"compress ", b"decompress ", b"burrows ", b"wheeler ", b"transform ",
        ];
        let mut x: u64 = 7;
        for _ in 0..80_000 {
            x = x.wrapping_mul(2862933555777941757).wrapping_add(3037000493);
            d.extend_from_slice(words[(x >> 33) as usize % words.len()]);
        }
        d
    }));
    /* upstream fixture files, if present (repo root, else ../bzip2) */
    for f in ["sample1.ref", "sample2.ref", "sample3.ref", "words0", "words1"] {
        for dir in [crate_root(), crate_root().join("../bzip2")] {
            if let Ok(data) = std::fs::read(dir.join(f)) {
                v.push((format!("file:{}", f), data));
                break;
            }
        }
    }
    v
}

#[test]
fn byte_exact_vs_c_oracle() {
    assert!(
        Path::new(&c_bzip2()).exists(),
        "C oracle not found at {:?}",
        c_bzip2()
    );
    for (name, src) in corpus() {
        for bs in [1, 4, 9] {
            check_one(&name, &src, bs);
        }
    }
}
