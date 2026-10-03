/*-----------------------------------------------------------*/
/*--- A block-sorting, lossless compressor        bzip2.c ---*/
/*---                       ported to Rust, same behavior   ---*/
/*-----------------------------------------------------------*/

/* ------------------------------------------------------------------
   This file is part of the Rust port of bzip2/libbzip2, a program and
   library for lossless, block-sorting data compression.

   bzip2/libbzip2 version 1.0.8 of 13 July 2019
   Copyright (C) 1996-2019 Julian Seward <jseward@acm.org>

   Please read the WARNING, DISCLAIMER and PATENTS sections in the
   README file.

   This program is released under the terms of the license contained
   in the file LICENSE.
   ------------------------------------------------------------------ */

/* Literal translation of bzip2.c, configured for BZ_UNIX.
   Global state and control flow mirror the original C. */

#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(static_mut_refs)]
/* literal C port: C-format `b"...\0"` literals and macro-shaped
   control flow are intentional */
#![allow(clippy::manual_c_str_literals)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::if_same_then_else)]
#![allow(clippy::unnecessary_cast)]
#![allow(clippy::needless_range_loop)]
#![allow(clippy::manual_clamp)]
#![allow(clippy::comparison_chain)]

use bz2::sys;
use bz2::*;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};
use std::os::unix::ffi::OsStrExt;

const FILE_NAME_LEN: usize = 1034;

const SM_I2O: i32 = 1;
const SM_F2O: i32 = 2;
const SM_F2F: i32 = 3;

const OM_Z: i32 = 1;
const OM_UNZ: i32 = 2;
const OM_TEST: i32 = 3;

/*-- saved metadata of the input file (the C `fileMetaInfo`) --*/
#[derive(Clone, Copy, Default)]
struct FileMetaInfo {
    st_mode: u32,
    st_uid: u32,
    st_gid: u32,
    st_atime: i64,
    st_mtime: i64,
}

struct Glob {
    verbosity: i32,
    keep_input_files: Bool,
    small_mode: Bool,
    delete_output_on_interrupt: Bool,
    force_overwrite: Bool,
    test_fails_exist: Bool,
    unz_fails_exist: Bool,
    noisy: Bool,
    num_file_names: i32,
    num_files_processed: i32,
    block_size_100k: i32,
    exit_value: i32,
    op_mode: i32,
    src_mode: i32,
    longest_file_name: i32,
    in_name: [c_char; FILE_NAME_LEN],
    out_name: [c_char; FILE_NAME_LEN],
    prog_name_really: [c_char; FILE_NAME_LEN],
    output_handle_just_in_case: *mut sys::FILE,
    work_factor: i32,
    file_meta_info: FileMetaInfo,
}

static mut G: Glob = Glob {
    verbosity: 0,
    keep_input_files: FALSE,
    small_mode: FALSE,
    delete_output_on_interrupt: FALSE,
    force_overwrite: FALSE,
    test_fails_exist: FALSE,
    unz_fails_exist: FALSE,
    noisy: TRUE,
    num_file_names: 0,
    num_files_processed: 0,
    block_size_100k: 0,
    exit_value: 0,
    op_mode: 0,
    src_mode: 0,
    longest_file_name: 0,
    in_name: [0; FILE_NAME_LEN],
    out_name: [0; FILE_NAME_LEN],
    prog_name_really: [0; FILE_NAME_LEN],
    output_handle_just_in_case: std::ptr::null_mut(),
    work_factor: 0,
    file_meta_info: FileMetaInfo {
        st_mode: 0,
        st_uid: 0,
        st_gid: 0,
        st_atime: 0,
        st_mtime: 0,
    },
};

#[inline(always)]
#[allow(clippy::redundant_closure_call)]
fn g() -> &'static mut Glob {
    unsafe { &mut *std::ptr::addr_of_mut!(G) }
}

/* progName points into progNameReally past the last PATH_SEP in C;
   we keep it as a separate byte string (the tail). PROG_NAME is the
   raw name; PROG_NAME_C is the NUL-terminated CString whose pointer
   is handed to %s/perror. */
static mut PROG_NAME: Vec<u8> = Vec::new();
static mut PROG_NAME_C: Vec<u8> = Vec::new(); /* includes trailing NUL */

fn prog_name() -> *const c_char {
    unsafe { PROG_NAME_C.as_ptr() as *const c_char }
}
fn prog_name_bytes() -> &'static [u8] {
    unsafe { &PROG_NAME }
}

/*---------------------------------------------------*/
/*--- helpers over the fixed C char arrays        ---*/
/*---------------------------------------------------*/

fn c_strlen(p: *const c_char) -> usize {
    unsafe { CStr::from_ptr(p).to_bytes().len() }
}

fn c_bytes(p: *const c_char) -> Vec<u8> {
    unsafe { CStr::from_ptr(p).to_bytes().to_vec() }
}

fn c_copy_from_bytes(dst: *mut c_char, cap: usize, src: &[u8]) {
    /* strncpy + explicit NUL, as copyFileName does */
    unsafe {
        let n = src.len().min(cap);
        std::ptr::copy_nonoverlapping(src.as_ptr(), dst as *mut u8, n);
        *dst.add(n) = 0;
    }
}

fn c_append_str(dst: *mut c_char, cap: usize, tail: &[u8]) {
    /* strcat semantics */
    unsafe {
        let cur = c_strlen(dst);
        let room = cap - cur - 1;
        let n = tail.len().min(room);
        std::ptr::copy_nonoverlapping(tail.as_ptr(), (dst as *mut u8).add(cur), n);
        *dst.add(cur + n) = 0;
    }
}

fn c_eq_flag(a: &[u8], flag: &[u8]) -> bool {
    a == flag
}

/* fprintf(stderr, "...%s...", progName, inName) style helpers */
fn fmt_2s(fmt: &[u8], a: &[u8], b: &[u8]) {
    let f = CString::new(fmt).unwrap();
    let ca = CString::new(a).unwrap_or_else(|_| CString::new("?").unwrap());
    let cb = CString::new(b).unwrap_or_else(|_| CString::new("?").unwrap());
    unsafe {
        sys::fprintf(sys::stderr, f.as_ptr(), ca.as_ptr(), cb.as_ptr());
    }
}

fn fmt_1s(fmt: &str, a: &[u8]) {
    let f = CString::new(fmt).unwrap();
    let ca = CString::new(a).unwrap_or_else(|_| CString::new("?").unwrap());
    unsafe {
        sys::fprintf(sys::stderr, f.as_ptr(), ca.as_ptr());
    }
}

fn fmt_3s(fmt: &[u8], a: &[u8], b: &[u8], c: &[u8]) {
    let f = CString::new(fmt).unwrap();
    let ca = CString::new(a).unwrap_or_else(|_| CString::new("?").unwrap());
    let cb = CString::new(b).unwrap_or_else(|_| CString::new("?").unwrap());
    let cc = CString::new(c).unwrap_or_else(|_| CString::new("?").unwrap());
    unsafe {
        sys::fprintf(sys::stderr, f.as_ptr(), ca.as_ptr(), cb.as_ptr(), cc.as_ptr());
    }
}

/*---------------------------------------------------*/
/*--- Processing of complete files and streams    ---*/
/*---------------------------------------------------*/

unsafe fn myfeof(f: *mut sys::FILE) -> Bool {
    let c = sys::fgetc(f);
    if c == sys::EOF {
        return TRUE;
    }
    sys::ungetc(c, f);
    FALSE
}

unsafe fn compressStream(stream: *mut sys::FILE, zStream: *mut sys::FILE) {
    let mut ibuf = [0u8; 5000];
    let mut nbytes_in_lo32: u32 = 0;
    let mut nbytes_in_hi32: u32 = 0;
    let mut nbytes_out_lo32: u32 = 0;
    let mut nbytes_out_hi32: u32 = 0;
    let mut bzerr: c_int = 0;

    if sys::ferror(stream) != 0 || sys::ferror(zStream) != 0 {
        ioError();
    }

    let bzf: *mut BZFILE = BZ2_bzWriteOpen(
        &mut bzerr,
        zStream,
        g().block_size_100k,
        g().verbosity,
        g().work_factor,
    );
    if bzerr != BZ_OK {
        goto_errhandler(bzf, bzerr);
    }

    if g().verbosity >= 2 {
        sys::fprintf(sys::stderr, b"\n\0".as_ptr() as *const c_char);
    }

    loop {
        if myfeof(stream) != 0 {
            break;
        }
        let n_ibuf = sys::fread(
            ibuf.as_mut_ptr() as *mut c_void,
            1,
            5000,
            stream,
        ) as i32;
        if sys::ferror(stream) != 0 {
            ioError();
        }
        if n_ibuf > 0 {
            BZ2_bzWrite(
                &mut bzerr,
                bzf,
                ibuf.as_mut_ptr() as *mut c_void,
                n_ibuf,
            );
        }
        if bzerr != BZ_OK {
            goto_errhandler(bzf, bzerr);
        }
    }

    BZ2_bzWriteClose64(
        &mut bzerr,
        bzf,
        0,
        &mut nbytes_in_lo32,
        &mut nbytes_in_hi32,
        &mut nbytes_out_lo32,
        &mut nbytes_out_hi32,
    );
    if bzerr != BZ_OK {
        goto_errhandler(bzf, bzerr);
    }

    if sys::ferror(zStream) != 0 {
        ioError();
    }
    if sys::fflush(zStream) == sys::EOF {
        ioError();
    }
    if zStream != sys::stdout {
        let fd = sys::fileno(zStream);
        if fd < 0 {
            ioError();
        }
        applySavedFileAttrToOutputFile(fd);
        let ret = sys::fclose(zStream);
        g().output_handle_just_in_case = std::ptr::null_mut();
        if ret == sys::EOF {
            ioError();
        }
    }
    g().output_handle_just_in_case = std::ptr::null_mut();
    if sys::ferror(stream) != 0 {
        ioError();
    }
    if sys::fclose(stream) == sys::EOF {
        ioError();
    }

    if g().verbosity >= 1 {
        if nbytes_in_lo32 == 0 && nbytes_in_hi32 == 0 {
            sys::fprintf(sys::stderr, b" no data compressed.\n\0".as_ptr() as *const c_char);
        } else {
            let nbytes_in = (nbytes_in_lo32 as u64) | ((nbytes_in_hi32 as u64) << 32);
            let nbytes_out = (nbytes_out_lo32 as u64) | ((nbytes_out_hi32 as u64) << 32);
            let nbytes_in_d = nbytes_in as f64;
            let nbytes_out_d = nbytes_out as f64;
            let buf_nin = CString::new(nbytes_in.to_string()).unwrap();
            let buf_nout = CString::new(nbytes_out.to_string()).unwrap();
            sys::fprintf(
                sys::stderr,
                b"%6.3f:1, %6.3f bits/byte, %5.2f%% saved, %s in, %s out.\n\0".as_ptr()
                    as *const c_char,
                nbytes_in_d / nbytes_out_d,
                (8.0 * nbytes_out_d) / nbytes_in_d,
                100.0 * (1.0 - nbytes_out_d / nbytes_in_d),
                buf_nin.as_ptr(),
                buf_nout.as_ptr(),
            );
        }
    }

    unsafe fn goto_errhandler(bzf: *mut BZFILE, bzerr: c_int) -> ! {
        let mut dummy: c_int = 0;
        let mut lo: u32 = 0;
        let mut hi: u32 = 0;
        let mut lo2: u32 = 0;
        let mut hi2: u32 = 0;
        unsafe {
            BZ2_bzWriteClose64(&mut dummy, bzf, 1, &mut lo, &mut hi, &mut lo2, &mut hi2);
            match bzerr {
                BZ_CONFIG_ERROR => configError(),
                BZ_MEM_ERROR => outOfMemory(),
                BZ_IO_ERROR => ioError(),
                _ => panic(b"compress:unexpected error"),
            }
        }
        #[allow(unreachable_code)]
        panic(b"compress:end");
    }
}

unsafe fn uncompressStream(zStream: *mut sys::FILE, stream: *mut sys::FILE) -> Bool {
    let mut bzf: *mut BZFILE;
    let mut bzerr: c_int = 0;
    let mut obuf = [0u8; 5000];
    let mut unused = [0u8; BZ_MAX_UNUSED];
    let mut n_unused: i32 = 0;
    let mut stream_no: i32 = 0;

    if sys::ferror(stream) != 0 || sys::ferror(zStream) != 0 {
        ioError();
    }

    'outer: loop {
        bzf = BZ2_bzReadOpen(
            &mut bzerr,
            zStream,
            g().verbosity,
            g().small_mode as i32,
            unused.as_mut_ptr() as *mut c_void,
            n_unused,
        );
        if bzf.is_null() || bzerr != BZ_OK {
            return uncompress_errhandler(zStream, stream, bzf, bzerr, stream_no);
        }
        stream_no += 1;

        while bzerr == BZ_OK {
            let nread = BZ2_bzRead(
                &mut bzerr,
                bzf,
                obuf.as_mut_ptr() as *mut c_void,
                5000,
            );
            if bzerr == BZ_DATA_ERROR_MAGIC {
                /* trycat */
                if g().force_overwrite != 0 {
                    sys::rewind(zStream);
                    loop {
                        if myfeof(zStream) != 0 {
                            break;
                        }
                        let nread = sys::fread(
                            obuf.as_mut_ptr() as *mut c_void,
                            1,
                            5000,
                            zStream,
                        );
                        if sys::ferror(zStream) != 0 {
                            ioError();
                        }
                        if nread > 0 {
                            sys::fwrite(
                                obuf.as_ptr() as *const c_void,
                                1,
                                nread,
                                stream,
                            );
                        }
                        if sys::ferror(stream) != 0 {
                            ioError();
                        }
                    }
                    /* closeok */
                    if sys::ferror(zStream) != 0 {
                        ioError();
                    }
                    if stream != sys::stdout {
                        let fd = sys::fileno(stream);
                        if fd < 0 {
                            ioError();
                        }
                        applySavedFileAttrToOutputFile(fd);
                    }
                    if sys::fclose(zStream) == sys::EOF {
                        ioError();
                    }
                    if sys::ferror(stream) != 0 {
                        ioError();
                    }
                    if sys::fflush(stream) != 0 {
                        ioError();
                    }
                    if stream != sys::stdout {
                        let ret = sys::fclose(stream);
                        g().output_handle_just_in_case = std::ptr::null_mut();
                        if ret == sys::EOF {
                            ioError();
                        }
                    }
                    g().output_handle_just_in_case = std::ptr::null_mut();
                    if g().verbosity >= 2 {
                        sys::fprintf(sys::stderr, b"\n    \0".as_ptr() as *const c_char);
                    }
                    return TRUE;
                }
                return uncompress_errhandler(zStream, stream, bzf, bzerr, stream_no);
            }
            if (bzerr == BZ_OK || bzerr == BZ_STREAM_END) && nread > 0 {
                sys::fwrite(obuf.as_ptr() as *const c_void, 1, nread as usize, stream);
            }
            if sys::ferror(stream) != 0 {
                ioError();
            }
        }
        if bzerr != BZ_STREAM_END {
            return uncompress_errhandler(zStream, stream, bzf, bzerr, stream_no);
        }

        let mut unused_tmp_v: *mut c_void = std::ptr::null_mut();
        BZ2_bzReadGetUnused(&mut bzerr, bzf, &mut unused_tmp_v, &mut n_unused);
        if bzerr != BZ_OK {
            panic(b"decompress:bzReadGetUnused");
        }

        let unused_tmp = unused_tmp_v as *mut u8;
        for i in 0..n_unused as usize {
            unused[i] = *unused_tmp.add(i);
        }

        BZ2_bzReadClose(&mut bzerr, bzf);
        if bzerr != BZ_OK {
            panic(b"decompress:bzReadGetUnused");
        }

        if n_unused == 0 && myfeof(zStream) != 0 {
            break 'outer;
        }
    }

    /* closeok */
    if sys::ferror(zStream) != 0 {
        ioError();
    }
    if stream != sys::stdout {
        let fd = sys::fileno(stream);
        if fd < 0 {
            ioError();
        }
        applySavedFileAttrToOutputFile(fd);
    }
    if sys::fclose(zStream) == sys::EOF {
        ioError();
    }

    if sys::ferror(stream) != 0 {
        ioError();
    }
    if sys::fflush(stream) != 0 {
        ioError();
    }
    if stream != sys::stdout {
        let ret = sys::fclose(stream);
        g().output_handle_just_in_case = std::ptr::null_mut();
        if ret == sys::EOF {
            ioError();
        }
    }
    g().output_handle_just_in_case = std::ptr::null_mut();
    if g().verbosity >= 2 {
        sys::fprintf(sys::stderr, b"\n    \0".as_ptr() as *const c_char);
    }
    TRUE
}

/* errhandler of uncompressStream: diverges for fatal errors, and
   returns False/True for BZ_DATA_ERROR_MAGIC exactly like the C. */
unsafe fn uncompress_errhandler(
    zStream: *mut sys::FILE,
    stream: *mut sys::FILE,
    bzf: *mut BZFILE,
    bzerr: c_int,
    stream_no: i32,
) -> Bool {
    let mut dummy: c_int = 0;
    BZ2_bzReadClose(&mut dummy, bzf);
    match bzerr {
        BZ_CONFIG_ERROR => configError(),
        BZ_IO_ERROR => ioError(),
        BZ_DATA_ERROR => crcError(),
        BZ_MEM_ERROR => outOfMemory(),
        BZ_UNEXPECTED_EOF => compressedStreamEOF(),
        BZ_DATA_ERROR_MAGIC => {
            if zStream != sys::stdin {
                sys::fclose(zStream);
            }
            if stream != sys::stdout {
                sys::fclose(stream);
            }
            if stream_no == 1 {
                FALSE
            } else {
                if g().noisy != 0 {
                    fmt_2s(
                        b"\n%s: %s: trailing garbage after EOF ignored\n",
                        prog_name_bytes(),
                        &c_bytes(g().in_name.as_ptr()),
                    );
                }
                TRUE
            }
        }
        _ => panic(b"decompress:unexpected error"),
    }
}

unsafe fn testStream(zStream: *mut sys::FILE) -> Bool {
    let mut bzf: *mut BZFILE;
    let mut bzerr: c_int = 0;
    let mut obuf = [0u8; 5000];
    let mut unused = [0u8; BZ_MAX_UNUSED];
    let mut n_unused: i32 = 0;
    let mut stream_no: i32 = 0;

    if sys::ferror(zStream) != 0 {
        ioError();
    }

    'outer: loop {
        bzf = BZ2_bzReadOpen(
            &mut bzerr,
            zStream,
            g().verbosity,
            g().small_mode as i32,
            unused.as_mut_ptr() as *mut c_void,
            n_unused,
        );
        if bzf.is_null() || bzerr != BZ_OK {
            return test_errhandler(zStream, bzf, bzerr, stream_no);
        }
        stream_no += 1;

        while bzerr == BZ_OK {
            BZ2_bzRead(&mut bzerr, bzf, obuf.as_mut_ptr() as *mut c_void, 5000);
            if bzerr == BZ_DATA_ERROR_MAGIC {
                return test_errhandler(zStream, bzf, bzerr, stream_no);
            }
        }
        if bzerr != BZ_STREAM_END {
            return test_errhandler(zStream, bzf, bzerr, stream_no);
        }

        let mut unused_tmp_v: *mut c_void = std::ptr::null_mut();
        BZ2_bzReadGetUnused(&mut bzerr, bzf, &mut unused_tmp_v, &mut n_unused);
        if bzerr != BZ_OK {
            panic(b"test:bzReadGetUnused");
        }

        let unused_tmp = unused_tmp_v as *mut u8;
        for i in 0..n_unused as usize {
            unused[i] = *unused_tmp.add(i);
        }

        BZ2_bzReadClose(&mut bzerr, bzf);
        if bzerr != BZ_OK {
            panic(b"test:bzReadGetUnused");
        }
        if n_unused == 0 && myfeof(zStream) != 0 {
            break 'outer;
        }
    }

    if sys::ferror(zStream) != 0 {
        ioError();
    }
    if sys::fclose(zStream) == sys::EOF {
        ioError();
    }

    if g().verbosity >= 2 {
        sys::fprintf(sys::stderr, b"\n    \0".as_ptr() as *const c_char);
    }
    TRUE
}

unsafe fn test_errhandler(
    zStream: *mut sys::FILE,
    bzf: *mut BZFILE,
    bzerr: c_int,
    stream_no: i32,
) -> Bool {
    let mut dummy: c_int = 0;
    BZ2_bzReadClose(&mut dummy, bzf);
    if g().verbosity == 0 {
        fmt_2s(b"%s: %s: ", prog_name_bytes(), &c_bytes(g().in_name.as_ptr()));
    }
    match bzerr {
        BZ_CONFIG_ERROR => configError(),
        BZ_IO_ERROR => ioError(),
        BZ_DATA_ERROR => {
            sys::fprintf(
                sys::stderr,
                b"data integrity (CRC) error in data\n\0".as_ptr() as *const c_char,
            );
            FALSE
        }
        BZ_MEM_ERROR => outOfMemory(),
        BZ_UNEXPECTED_EOF => {
            sys::fprintf(
                sys::stderr,
                b"file ends unexpectedly\n\0".as_ptr() as *const c_char,
            );
            FALSE
        }
        BZ_DATA_ERROR_MAGIC => {
            if zStream != sys::stdin {
                sys::fclose(zStream);
            }
            if stream_no == 1 {
                sys::fprintf(
                    sys::stderr,
                    b"bad magic number (file not created by bzip2)\n\0".as_ptr()
                        as *const c_char,
                );
                FALSE
            } else {
                if g().noisy != 0 {
                    sys::fprintf(
                        sys::stderr,
                        b"trailing garbage after EOF ignored\n\0".as_ptr() as *const c_char,
                    );
                }
                TRUE
            }
        }
        _ => panic(b"test:unexpected error"),
    }
}

/*---------------------------------------------------*/
/*--- Error [non-] handling grunge                ---*/
/*---------------------------------------------------*/

fn setExit(v: i32) {
    if v > g().exit_value {
        g().exit_value = v;
    }
}

fn cadvise() {
    unsafe {
        if g().noisy != 0 {
            sys::fprintf(
                sys::stderr,
                b"\nIt is possible that the compressed file(s) have become corrupted.\nYou can use the -tvv option to test integrity of such files.\n\nYou can use the `bzip2recover' program to attempt to recover\ndata from undamaged sections of corrupted files.\n\n\0"
                    .as_ptr() as *const c_char,
            );
        }
    }
}

fn showFileNames() {
    unsafe {
        if g().noisy != 0 {
            sys::fprintf(
                sys::stderr,
                b"\tInput file = %s, output file = %s\n\0".as_ptr() as *const c_char,
                g().in_name.as_ptr(),
                g().out_name.as_ptr(),
            );
        }
    }
}

fn cleanUpAndFail(ec: i32) -> ! {
    unsafe {
        if g().src_mode == SM_F2F
            && g().op_mode != OM_TEST
            && g().delete_output_on_interrupt != 0
        {
            /* Check whether input file still exists.  Delete output file
               only if input exists to avoid loss of data. */
            let in_name_s = c_bytes(g().in_name.as_ptr());
            if std::fs::metadata(std::ffi::OsStr::from_bytes(&in_name_s)).is_ok() {
                if g().noisy != 0 {
                    fmt_2s(
                        b"%s: Deleting output file %s, if it exists.\n",
                        prog_name_bytes(),
                        &c_bytes(g().out_name.as_ptr()),
                    );
                }
                if !g().output_handle_just_in_case.is_null() {
                    sys::fclose(g().output_handle_just_in_case);
                }
                let ret_val = sys::remove(g().out_name.as_ptr());
                if ret_val != 0 {
                    fmt_1s(
                        "%s: WARNING: deletion of output file (apparently) failed.\n",
                        prog_name_bytes(),
                    );
                }
            } else {
                fmt_1s(
                    "%s: WARNING: deletion of output file suppressed\n",
                    prog_name_bytes(),
                );
                fmt_1s(
                    "%s:    since input file no longer exists.  Output file\n",
                    prog_name_bytes(),
                );
                fmt_2s(
                    b"%s:    `%s' may be incomplete.\n",
                    prog_name_bytes(),
                    &c_bytes(g().out_name.as_ptr()),
                );
                fmt_1s(
                    "%s:    I suggest doing an integrity test (bzip2 -tv) of it.\n",
                    prog_name_bytes(),
                );
            }
        }

        if g().noisy != 0
            && g().num_file_names > 0
            && g().num_files_processed < g().num_file_names
        {
            sys::fprintf(
                sys::stderr,
                b"%s: WARNING: some files have not been processed:\n%s:    %d specified on command line, %d not processed yet.\n\n\0"
                    .as_ptr() as *const c_char,
                prog_name(),
                prog_name(),
                g().num_file_names,
                g().num_file_names - g().num_files_processed,
            );
        }
        setExit(ec);
        sys::exit(g().exit_value);
    }
}

fn panic(s: &[u8]) -> ! {
    unsafe {
        let cs = CString::new(s).unwrap();
        sys::fprintf(
            sys::stderr,
            b"\n%s: PANIC -- internal consistency error:\n\t%s\n\tThis is a BUG.  Please report it to:\n\tbzip2-devel@sourceware.org\n\0"
                .as_ptr() as *const c_char,
            prog_name(),
            cs.as_ptr(),
        );
    }
    showFileNames();
    cleanUpAndFail(3);
}

fn crcError() -> ! {
    fmt_1s(
        "\n%s: Data integrity error when decompressing.\n",
        prog_name_bytes(),
    );
    showFileNames();
    cadvise();
    cleanUpAndFail(2);
}

fn compressedStreamEOF() -> ! {
    unsafe {
        if g().noisy != 0 {
            fmt_1s(
                "\n%s: Compressed file ends unexpectedly;\n\tperhaps it is corrupted?  *Possible* reason follows.\n",
                prog_name_bytes(),
            );
            sys::perror(prog_name());
            showFileNames();
            cadvise();
        }
        cleanUpAndFail(2);
    }
}

fn ioError() -> ! {
    fmt_1s(
        "\n%s: I/O or other error, bailing out.  Possible reason follows.\n",
        prog_name_bytes(),
    );
    unsafe {
        sys::perror(prog_name());
    }
    showFileNames();
    cleanUpAndFail(1);
}

extern "C" fn mySignalCatcher(_n: c_int) {
    fmt_1s(
        "\n%s: Control-C or similar caught, quitting.\n",
        prog_name_bytes(),
    );
    cleanUpAndFail(1);
}

extern "C" fn mySIGSEGVorSIGBUScatcher(_n: c_int) {
    unsafe {
        if g().op_mode == OM_Z {
            sys::fprintf(
                sys::stderr,
                b"\n%s: Caught a SIGSEGV or SIGBUS whilst compressing.\n\n   Possible causes are (most likely first):\n   (1) This computer has unreliable memory or cache hardware\n       (a surprisingly common problem; try a different machine.)\n   (2) A bug in the compiler used to create this executable\n       (unlikely, if you didn't compile bzip2 yourself.)\n   (3) A real bug in bzip2 -- I hope this should never be the case.\n   The user's manual, Section 4.3, has more info on (1) and (2).\n   \n   If you suspect this is a bug in bzip2, or are unsure about (1)\n   or (2), feel free to report it to: bzip2-devel@sourceware.org.\n   Section 4.3 of the user's manual describes the info a useful\n   bug report should have.  If the manual is available on your\n   system, please try and read it before mailing me.  If you don't\n   have the manual or can't be bothered to read it, mail me anyway.\n\n\0"
                    .as_ptr() as *const c_char,
                prog_name(),
            );
        } else {
            sys::fprintf(
                sys::stderr,
                b"\n%s: Caught a SIGSEGV or SIGBUS whilst decompressing.\n\n   Possible causes are (most likely first):\n   (1) The compressed data is corrupted, and bzip2's usual checks\n       failed to detect this.  Try bzip2 -tvv my_file.bz2.\n   (2) This computer has unreliable memory or cache hardware\n       (a surprisingly common problem; try a different machine.)\n   (3) A bug in the compiler used to create this executable\n       (unlikely, if you didn't compile bzip2 yourself.)\n   (4) A real bug in bzip2 -- I hope this should never be the case.\n   The user's manual, Section 4.3, has more info on (2) and (3).\n   \n   If you suspect this is a bug in bzip2, or are unsure about (2)\n   or (3), feel free to report it to: bzip2-devel@sourceware.org.\n   Section 4.3 of the user's manual describes the info a useful\n   bug report should have.  If the manual is available on your\n   system, please try and read it before mailing me.  If you don't\n   have the manual or can't be bothered to read it, mail me anyway.\n\n\0"
                    .as_ptr() as *const c_char,
                prog_name(),
            );
        }
    }
    showFileNames();
    if g().op_mode == OM_Z {
        cleanUpAndFail(3);
    } else {
        cadvise();
        cleanUpAndFail(2);
    }
}

fn outOfMemory() -> ! {
    fmt_1s("\n%s: couldn't allocate enough memory\n", prog_name_bytes());
    showFileNames();
    cleanUpAndFail(1);
}

fn configError() -> ! {
    unsafe {
        sys::fprintf(
            sys::stderr,
            b"bzip2: I'm not configured correctly for this platform!\n\tI require Int32, Int16 and Char to have sizes\n\tof 4, 2 and 1 bytes to run properly, and they don't.\n\tProbably you can fix this by defining them correctly,\n\tand recompiling.  Bye!\n\0"
                .as_ptr() as *const c_char,
        );
    }
    setExit(3);
    unsafe { sys::exit(g().exit_value) }
}

/*---------------------------------------------------*/
/*--- The main driver machinery                   ---*/
/*---------------------------------------------------*/

fn pad(s: *const c_char) {
    unsafe {
        if (c_strlen(s) as i32) >= g().longest_file_name {
            return;
        }
        for _ in 1..=(g().longest_file_name - c_strlen(s) as i32) {
            sys::fprintf(sys::stderr, b" \0".as_ptr() as *const c_char);
        }
    }
}

fn copyFileName(to: *mut c_char, from: &[u8]) {
    if from.len() > FILE_NAME_LEN - 10 {
        let cf = CString::new(from).unwrap_or_else(|_| CString::new("?").unwrap());
        unsafe {
            sys::fprintf(
                sys::stderr,
                b"bzip2: file name\n`%s'\nis suspiciously (more than %d chars) long.\nTry using a reasonable file name instead.  Sorry! :-)\n\0"
                    .as_ptr() as *const c_char,
                cf.as_ptr(),
                (FILE_NAME_LEN - 10) as c_int,
            );
        }
        setExit(1);
        unsafe { sys::exit(g().exit_value) }
    }
    c_copy_from_bytes(to, FILE_NAME_LEN - 10, from);
}

fn fileExists(name: *const c_char) -> Bool {
    unsafe {
        let tmp = sys::fopen(name, b"rb\0".as_ptr() as *const c_char);
        let exists = !tmp.is_null();
        if exists {
            sys::fclose(tmp);
        }
        exists as Bool
    }
}

/* Open an output file safely with O_EXCL and good permissions. */
fn fopen_output_safely(name: *const c_char, mode: *const c_char) -> *mut sys::FILE {
    unsafe {
        let fh = sys::open(
            name,
            sys::O_WRONLY | sys::O_CREAT | sys::O_EXCL,
            sys::S_IWUSR | sys::S_IRUSR,
        );
        if fh == -1 {
            return std::ptr::null_mut();
        }
        let fp = sys::fdopen(fh, mode);
        if fp.is_null() {
            sys::close(fh);
        }
        fp
    }
}

/*-- if in doubt, return True --*/
fn notAStandardFile(name: *const c_char) -> Bool {
    let name_b = c_bytes(name);
    match std::fs::symlink_metadata(std::ffi::OsStr::from_bytes(&name_b)) {
        Err(_) => TRUE,
        Ok(m) => {
            if m.file_type().is_file() {
                FALSE
            } else {
                TRUE
            }
        }
    }
}

/*-- see if file has hard links to it --*/
fn countHardLinks(name: *const c_char) -> i32 {
    use std::os::unix::fs::MetadataExt;
    let name_b = c_bytes(name);
    match std::fs::symlink_metadata(std::ffi::OsStr::from_bytes(&name_b)) {
        Err(_) => 0,
        Ok(m) => m.nlink() as i32 - 1,
    }
}

/* Copy modification date, access date, permissions and owner from the
   source to destination file.  We have to copy this meta-info off
   into fileMetaInfo before starting to compress / decompress it,
   because doing it afterwards means we get the wrong access time. */
fn saveInputFileMetaInfo(srcName: *const c_char) {
    use std::os::unix::fs::MetadataExt;
    let name_b = c_bytes(srcName);
    /* Note use of stat here, not lstat. */
    match std::fs::metadata(std::ffi::OsStr::from_bytes(&name_b)) {
        Ok(m) => {
            g().file_meta_info = FileMetaInfo {
                st_mode: m.mode(),
                st_uid: m.uid(),
                st_gid: m.gid(),
                st_atime: m.atime(),
                st_mtime: m.mtime(),
            };
        }
        Err(_) => ioError(),
    }
}

fn applySavedTimeInfoToOutputFile(dstName: *const c_char) {
    unsafe {
        let u_tim_buf = sys::utimbuf {
            actime: g().file_meta_info.st_atime,
            modtime: g().file_meta_info.st_mtime,
        };
        if sys::utime(dstName, &u_tim_buf) != 0 {
            ioError();
        }
    }
}

fn applySavedFileAttrToOutputFile(fd: c_int) {
    unsafe {
        if sys::fchmod(fd, g().file_meta_info.st_mode) != 0 {
            ioError();
        }
        let _ = sys::fchown(fd, g().file_meta_info.st_uid, g().file_meta_info.st_gid);
        /* chown() will in many cases return with EPERM, which can
           be safely ignored. */
    }
}

fn containsDubiousChars(_name: *const c_char) -> Bool {
    /* On unix, files can contain any characters and the file
       expansion is performed by the shell. */
    FALSE
}

const BZ_N_SUFFIX_PAIRS: usize = 4;
const Z_SUFFIX: [&[u8]; BZ_N_SUFFIX_PAIRS] = [b".bz2", b".bz", b".tbz2", b".tbz"];
const UNZ_SUFFIX: [&[u8]; BZ_N_SUFFIX_PAIRS] = [b"", b"", b".tar", b".tar"];

fn hasSuffix(s: &[u8], suffix: &[u8]) -> bool {
    let ns = s.len();
    let nx = suffix.len();
    if ns < nx {
        return false;
    }
    &s[ns - nx..] == suffix
}

/* operates on the out_name global char array, like C */
fn mapSuffix(name: *mut c_char, old_suffix: &[u8], new_suffix: &[u8]) -> Bool {
    let cur = c_bytes(name);
    if !hasSuffix(&cur, old_suffix) {
        return FALSE;
    }
    let keep = cur.len() - old_suffix.len();
    let mut nv = cur[..keep].to_vec();
    nv.extend_from_slice(new_suffix);
    c_copy_from_bytes(name, FILE_NAME_LEN - 1, &nv);
    TRUE
}

fn compress(name: Option<&[u8]>) {
    unsafe {
        g().delete_output_on_interrupt = FALSE;

        if name.is_none() && g().src_mode != SM_I2O {
            panic(b"compress: bad modes\n");
        }

        match g().src_mode {
            SM_I2O => {
                copyFileName(g().in_name.as_mut_ptr(), b"(stdin)");
                copyFileName(g().out_name.as_mut_ptr(), b"(stdout)");
            }
            SM_F2F => {
                copyFileName(g().in_name.as_mut_ptr(), name.unwrap());
                copyFileName(g().out_name.as_mut_ptr(), name.unwrap());
                c_append_str(g().out_name.as_mut_ptr(), FILE_NAME_LEN, b".bz2");
            }
            SM_F2O => {
                copyFileName(g().in_name.as_mut_ptr(), name.unwrap());
                copyFileName(g().out_name.as_mut_ptr(), b"(stdout)");
            }
            _ => {}
        }

        if g().src_mode != SM_I2O && containsDubiousChars(g().in_name.as_ptr()) != 0 {
            if g().noisy != 0 {
                fmt_2s(
                    b"%s: There are no files matching `%s'.\n",
                    prog_name_bytes(),
                    &c_bytes(g().in_name.as_ptr()),
                );
            }
            setExit(1);
            return;
        }
        if g().src_mode != SM_I2O && fileExists(g().in_name.as_ptr()) == 0 {
            let e = sys::errno_val();
            fmt_2s_errno(
                b"%s: Can't open input file %s: %s.\n",
                prog_name_bytes(),
                &c_bytes(g().in_name.as_ptr()),
                e,
            );
            setExit(1);
            return;
        }
        for i in 0..BZ_N_SUFFIX_PAIRS {
            if hasSuffix(&c_bytes(g().in_name.as_ptr()), Z_SUFFIX[i]) {
                if g().noisy != 0 {
                    sys::fprintf(
                        sys::stderr,
                        b"%s: Input file %s already has %s suffix.\n\0".as_ptr()
                            as *const c_char,
                        prog_name(),
                        g().in_name.as_ptr(),
                        CString::new(Z_SUFFIX[i]).unwrap().as_ptr(),
                    );
                }
                setExit(1);
                return;
            }
        }
        if g().src_mode == SM_F2F || g().src_mode == SM_F2O {
            let in_b = c_bytes(g().in_name.as_ptr());
            if let Ok(m) = std::fs::metadata(std::ffi::OsStr::from_bytes(&in_b)) {
                if m.file_type().is_dir() {
                    sys::fprintf(
                        sys::stderr,
                        b"%s: Input file %s is a directory.\n\0".as_ptr() as *const c_char,
                        prog_name(),
                        g().in_name.as_ptr(),
                    );
                    setExit(1);
                    return;
                }
            }
        }
        if g().src_mode == SM_F2F
            && g().force_overwrite == 0
            && notAStandardFile(g().in_name.as_ptr()) != 0
        {
            if g().noisy != 0 {
                fmt_2s(
                    b"%s: Input file %s is not a normal file.\n",
                    prog_name_bytes(),
                    &c_bytes(g().in_name.as_ptr()),
                );
            }
            setExit(1);
            return;
        }
        if g().src_mode == SM_F2F && fileExists(g().out_name.as_ptr()) != 0 {
            if g().force_overwrite != 0 {
                sys::remove(g().out_name.as_ptr());
            } else {
                fmt_2s(
                    b"%s: Output file %s already exists.\n",
                    prog_name_bytes(),
                    &c_bytes(g().out_name.as_ptr()),
                );
                setExit(1);
                return;
            }
        }
        if g().src_mode == SM_F2F && g().force_overwrite == 0 {
            let n = countHardLinks(g().in_name.as_ptr());
            if n > 0 {
                sys::fprintf(
                    sys::stderr,
                    b"%s: Input file %s has %d other link%s.\n\0".as_ptr() as *const c_char,
                    prog_name(),
                    g().in_name.as_ptr(),
                    n,
                    if n > 1 {
                        b"s\0".as_ptr() as *const c_char
                    } else {
                        b"\0".as_ptr() as *const c_char
                    },
                );
                setExit(1);
                return;
            }
        }

        if g().src_mode == SM_F2F {
            /* Save the file's meta-info before we open it. */
            saveInputFileMetaInfo(g().in_name.as_ptr());
        }

        let inStr: *mut sys::FILE;
        let outStr: *mut sys::FILE;
        match g().src_mode {
            SM_I2O => {
                inStr = sys::stdin;
                outStr = sys::stdout;
                if sys::isatty(sys::fileno(sys::stdout)) != 0 {
                    fmt_1s(
                        "%s: I won't write compressed data to a terminal.\n",
                        prog_name_bytes(),
                    );
                    fmt_2s(
                        b"%s: For help, type: `%s --help'.\n",
                        prog_name_bytes(),
                        prog_name_bytes(),
                    );
                    setExit(1);
                    return;
                }
            }
            SM_F2O => {
                inStr = sys::fopen(g().in_name.as_ptr(), b"rb\0".as_ptr() as *const c_char);
                outStr = sys::stdout;
                if sys::isatty(sys::fileno(sys::stdout)) != 0 {
                    fmt_1s(
                        "%s: I won't write compressed data to a terminal.\n",
                        prog_name_bytes(),
                    );
                    fmt_2s(
                        b"%s: For help, type: `%s --help'.\n",
                        prog_name_bytes(),
                        prog_name_bytes(),
                    );
                    if !inStr.is_null() {
                        sys::fclose(inStr);
                    }
                    setExit(1);
                    return;
                }
                if inStr.is_null() {
                    let e = sys::errno_val();
                    fmt_2s_errno(
                        b"%s: Can't open input file %s: %s.\n",
                        prog_name_bytes(),
                        &c_bytes(g().in_name.as_ptr()),
                        e,
                    );
                    setExit(1);
                    return;
                }
            }
            SM_F2F => {
                inStr = sys::fopen(g().in_name.as_ptr(), b"rb\0".as_ptr() as *const c_char);
                outStr =
                    fopen_output_safely(g().out_name.as_ptr(), b"wb\0".as_ptr() as *const c_char);
                if outStr.is_null() {
                    let e = sys::errno_val();
                    fmt_2s_errno(
                        b"%s: Can't create output file %s: %s.\n",
                        prog_name_bytes(),
                        &c_bytes(g().out_name.as_ptr()),
                        e,
                    );
                    if !inStr.is_null() {
                        sys::fclose(inStr);
                    }
                    setExit(1);
                    return;
                }
                if inStr.is_null() {
                    let e = sys::errno_val();
                    fmt_2s_errno(
                        b"%s: Can't open input file %s: %s.\n",
                        prog_name_bytes(),
                        &c_bytes(g().in_name.as_ptr()),
                        e,
                    );
                    sys::fclose(outStr);
                    setExit(1);
                    return;
                }
            }
            _ => panic(b"compress: bad srcMode"),
        }

        if g().verbosity >= 1 {
            sys::fprintf(
                sys::stderr,
                b"  %s: \0".as_ptr() as *const c_char,
                g().in_name.as_ptr(),
            );
            pad(g().in_name.as_ptr());
            sys::fflush(sys::stderr);
        }

        /*--- Now the input and output handles are sane.  Do the Biz. ---*/
        g().output_handle_just_in_case = outStr;
        g().delete_output_on_interrupt = TRUE;
        compressStream(inStr, outStr);
        g().output_handle_just_in_case = std::ptr::null_mut();

        /*--- If there was an I/O error, we won't get here. ---*/
        if g().src_mode == SM_F2F {
            applySavedTimeInfoToOutputFile(g().out_name.as_ptr());
            g().delete_output_on_interrupt = FALSE;
            if g().keep_input_files == 0 {
                if sys::remove(g().in_name.as_ptr()) != 0 {
                    ioError();
                }
            }
        }

        g().delete_output_on_interrupt = FALSE;
    }
}

fn fmt_2s_errno(fmt: &[u8], a: &[u8], b: &[u8], errnum: c_int) {
    /* "%s: msg %s: %s.\n" where third is strerror(errnum) */
    let f = CString::new(fmt).unwrap();
    let ca = CString::new(a).unwrap_or_else(|_| CString::new("?").unwrap());
    let cb = CString::new(b).unwrap_or_else(|_| CString::new("?").unwrap());
    unsafe {
        let es = sys::strerror(errnum);
        sys::fprintf(sys::stderr, f.as_ptr(), ca.as_ptr(), cb.as_ptr(), es);
    }
}

fn uncompress(name: Option<&[u8]>) {
    unsafe {
        g().delete_output_on_interrupt = FALSE;

        if name.is_none() && g().src_mode != SM_I2O {
            panic(b"uncompress: bad modes\n");
        }

        let mut cant_guess = false;
        match g().src_mode {
            SM_I2O => {
                copyFileName(g().in_name.as_mut_ptr(), b"(stdin)");
                copyFileName(g().out_name.as_mut_ptr(), b"(stdout)");
            }
            SM_F2F => {
                copyFileName(g().in_name.as_mut_ptr(), name.unwrap());
                copyFileName(g().out_name.as_mut_ptr(), name.unwrap());
                let mut mapped = false;
                for i in 0..BZ_N_SUFFIX_PAIRS {
                    if mapSuffix(g().out_name.as_mut_ptr(), Z_SUFFIX[i], UNZ_SUFFIX[i]) != 0 {
                        mapped = true;
                        break;
                    }
                }
                if !mapped {
                    cant_guess = true;
                    c_append_str(g().out_name.as_mut_ptr(), FILE_NAME_LEN, b".out");
                }
            }
            SM_F2O => {
                copyFileName(g().in_name.as_mut_ptr(), name.unwrap());
                copyFileName(g().out_name.as_mut_ptr(), b"(stdout)");
            }
            _ => {}
        }

        if g().src_mode != SM_I2O && containsDubiousChars(g().in_name.as_ptr()) != 0 {
            if g().noisy != 0 {
                fmt_2s(
                    b"%s: There are no files matching `%s'.\n",
                    prog_name_bytes(),
                    &c_bytes(g().in_name.as_ptr()),
                );
            }
            setExit(1);
            return;
        }
        if g().src_mode != SM_I2O && fileExists(g().in_name.as_ptr()) == 0 {
            let e = sys::errno_val();
            fmt_2s_errno(
                b"%s: Can't open input file %s: %s.\n",
                prog_name_bytes(),
                &c_bytes(g().in_name.as_ptr()),
                e,
            );
            setExit(1);
            return;
        }
        if g().src_mode == SM_F2F || g().src_mode == SM_F2O {
            let in_b = c_bytes(g().in_name.as_ptr());
            if let Ok(m) = std::fs::metadata(std::ffi::OsStr::from_bytes(&in_b)) {
                if m.file_type().is_dir() {
                    sys::fprintf(
                        sys::stderr,
                        b"%s: Input file %s is a directory.\n\0".as_ptr() as *const c_char,
                        prog_name(),
                        g().in_name.as_ptr(),
                    );
                    setExit(1);
                    return;
                }
            }
        }
        if g().src_mode == SM_F2F
            && g().force_overwrite == 0
            && notAStandardFile(g().in_name.as_ptr()) != 0
        {
            if g().noisy != 0 {
                fmt_2s(
                    b"%s: Input file %s is not a normal file.\n",
                    prog_name_bytes(),
                    &c_bytes(g().in_name.as_ptr()),
                );
            }
            setExit(1);
            return;
        }
        if cant_guess {
            if g().noisy != 0 {
                fmt_3s(
                    b"%s: Can't guess original name for %s -- using %s\n",
                    prog_name_bytes(),
                    &c_bytes(g().in_name.as_ptr()),
                    &c_bytes(g().out_name.as_ptr()),
                );
            }
            /* just a warning, no return */
        }
        if g().src_mode == SM_F2F && fileExists(g().out_name.as_ptr()) != 0 {
            if g().force_overwrite != 0 {
                sys::remove(g().out_name.as_ptr());
            } else {
                fmt_2s(
                    b"%s: Output file %s already exists.\n",
                    prog_name_bytes(),
                    &c_bytes(g().out_name.as_ptr()),
                );
                setExit(1);
                return;
            }
        }
        if g().src_mode == SM_F2F && g().force_overwrite == 0 {
            let n = countHardLinks(g().in_name.as_ptr());
            if n > 0 {
                sys::fprintf(
                    sys::stderr,
                    b"%s: Input file %s has %d other link%s.\n\0".as_ptr() as *const c_char,
                    prog_name(),
                    g().in_name.as_ptr(),
                    n,
                    if n > 1 {
                        b"s\0".as_ptr() as *const c_char
                    } else {
                        b"\0".as_ptr() as *const c_char
                    },
                );
                setExit(1);
                return;
            }
        }

        if g().src_mode == SM_F2F {
            saveInputFileMetaInfo(g().in_name.as_ptr());
        }

        let inStr: *mut sys::FILE;
        let outStr: *mut sys::FILE;
        match g().src_mode {
            SM_I2O => {
                inStr = sys::stdin;
                outStr = sys::stdout;
                if sys::isatty(sys::fileno(sys::stdin)) != 0 {
                    fmt_1s(
                        "%s: I won't read compressed data from a terminal.\n",
                        prog_name_bytes(),
                    );
                    fmt_2s(
                        b"%s: For help, type: `%s --help'.\n",
                        prog_name_bytes(),
                        prog_name_bytes(),
                    );
                    setExit(1);
                    return;
                }
            }
            SM_F2O => {
                inStr = sys::fopen(g().in_name.as_ptr(), b"rb\0".as_ptr() as *const c_char);
                outStr = sys::stdout;
                if inStr.is_null() {
                    let e = sys::errno_val();
                    fmt_2s_errno(
                        b"%s: Can't open input file %s:%s.\n",
                        prog_name_bytes(),
                        &c_bytes(g().in_name.as_ptr()),
                        e,
                    );
                    setExit(1);
                    return;
                }
            }
            SM_F2F => {
                inStr = sys::fopen(g().in_name.as_ptr(), b"rb\0".as_ptr() as *const c_char);
                outStr =
                    fopen_output_safely(g().out_name.as_ptr(), b"wb\0".as_ptr() as *const c_char);
                if outStr.is_null() {
                    let e = sys::errno_val();
                    fmt_2s_errno(
                        b"%s: Can't create output file %s: %s.\n",
                        prog_name_bytes(),
                        &c_bytes(g().out_name.as_ptr()),
                        e,
                    );
                    if !inStr.is_null() {
                        sys::fclose(inStr);
                    }
                    setExit(1);
                    return;
                }
                if inStr.is_null() {
                    let e = sys::errno_val();
                    fmt_2s_errno(
                        b"%s: Can't open input file %s: %s.\n",
                        prog_name_bytes(),
                        &c_bytes(g().in_name.as_ptr()),
                        e,
                    );
                    sys::fclose(outStr);
                    setExit(1);
                    return;
                }
            }
            _ => panic(b"uncompress: bad srcMode"),
        }

        if g().verbosity >= 1 {
            sys::fprintf(
                sys::stderr,
                b"  %s: \0".as_ptr() as *const c_char,
                g().in_name.as_ptr(),
            );
            pad(g().in_name.as_ptr());
            sys::fflush(sys::stderr);
        }

        /*--- Now the input and output handles are sane.  Do the Biz. ---*/
        g().output_handle_just_in_case = outStr;
        g().delete_output_on_interrupt = TRUE;
        let magic_number_ok = uncompressStream(inStr, outStr);
        g().output_handle_just_in_case = std::ptr::null_mut();

        /*--- If there was an I/O error, we won't get here. ---*/
        if magic_number_ok != 0 {
            if g().src_mode == SM_F2F {
                applySavedTimeInfoToOutputFile(g().out_name.as_ptr());
                g().delete_output_on_interrupt = FALSE;
                if g().keep_input_files == 0 {
                    if sys::remove(g().in_name.as_ptr()) != 0 {
                        ioError();
                    }
                }
            }
        } else {
            g().unz_fails_exist = TRUE;
            g().delete_output_on_interrupt = FALSE;
            if g().src_mode == SM_F2F {
                if sys::remove(g().out_name.as_ptr()) != 0 {
                    ioError();
                }
            }
        }
        g().delete_output_on_interrupt = FALSE;

        if magic_number_ok != 0 {
            if g().verbosity >= 1 {
                sys::fprintf(sys::stderr, b"done\n\0".as_ptr() as *const c_char);
            }
        } else {
            setExit(2);
            if g().verbosity >= 1 {
                sys::fprintf(sys::stderr, b"not a bzip2 file.\n\0".as_ptr() as *const c_char);
            } else {
                fmt_2s(
                    b"%s: %s is not a bzip2 file.\n",
                    prog_name_bytes(),
                    &c_bytes(g().in_name.as_ptr()),
                );
            }
        }
    }
}

fn testf(name: Option<&[u8]>) {
    unsafe {
        g().delete_output_on_interrupt = FALSE;

        if name.is_none() && g().src_mode != SM_I2O {
            panic(b"testf: bad modes\n");
        }

        copyFileName(g().out_name.as_mut_ptr(), b"(none)");
        match g().src_mode {
            SM_I2O => copyFileName(g().in_name.as_mut_ptr(), b"(stdin)"),
            _ => copyFileName(g().in_name.as_mut_ptr(), name.unwrap()),
        }

        if g().src_mode != SM_I2O && containsDubiousChars(g().in_name.as_ptr()) != 0 {
            if g().noisy != 0 {
                fmt_2s(
                    b"%s: There are no files matching `%s'.\n",
                    prog_name_bytes(),
                    &c_bytes(g().in_name.as_ptr()),
                );
            }
            setExit(1);
            return;
        }
        if g().src_mode != SM_I2O && fileExists(g().in_name.as_ptr()) == 0 {
            let e = sys::errno_val();
            fmt_2s_errno(
                b"%s: Can't open input %s: %s.\n",
                prog_name_bytes(),
                &c_bytes(g().in_name.as_ptr()),
                e,
            );
            setExit(1);
            return;
        }
        if g().src_mode != SM_I2O {
            let in_b = c_bytes(g().in_name.as_ptr());
            if let Ok(m) = std::fs::metadata(std::ffi::OsStr::from_bytes(&in_b)) {
                if m.file_type().is_dir() {
                    sys::fprintf(
                        sys::stderr,
                        b"%s: Input file %s is a directory.\n\0".as_ptr() as *const c_char,
                        prog_name(),
                        g().in_name.as_ptr(),
                    );
                    setExit(1);
                    return;
                }
            }
        }

        let inStr: *mut sys::FILE;
        match g().src_mode {
            SM_I2O => {
                if sys::isatty(sys::fileno(sys::stdin)) != 0 {
                    fmt_1s(
                        "%s: I won't read compressed data from a terminal.\n",
                        prog_name_bytes(),
                    );
                    fmt_2s(
                        b"%s: For help, type: `%s --help'.\n",
                        prog_name_bytes(),
                        prog_name_bytes(),
                    );
                    setExit(1);
                    return;
                }
                inStr = sys::stdin;
            }
            SM_F2O | SM_F2F => {
                inStr = sys::fopen(g().in_name.as_ptr(), b"rb\0".as_ptr() as *const c_char);
                if inStr.is_null() {
                    let e = sys::errno_val();
                    fmt_2s_errno(
                        b"%s: Can't open input file %s:%s.\n",
                        prog_name_bytes(),
                        &c_bytes(g().in_name.as_ptr()),
                        e,
                    );
                    setExit(1);
                    return;
                }
            }
            _ => panic(b"testf: bad srcMode"),
        }

        if g().verbosity >= 1 {
            sys::fprintf(
                sys::stderr,
                b"  %s: \0".as_ptr() as *const c_char,
                g().in_name.as_ptr(),
            );
            pad(g().in_name.as_ptr());
            sys::fflush(sys::stderr);
        }

        /*--- Now the input handle is sane.  Do the Biz. ---*/
        g().output_handle_just_in_case = std::ptr::null_mut();
        let all_ok = testStream(inStr);

        if all_ok != 0 && g().verbosity >= 1 {
            sys::fprintf(sys::stderr, b"ok\n\0".as_ptr() as *const c_char);
        }
        if all_ok == 0 {
            g().test_fails_exist = TRUE;
        }
    }
}

fn license() {
    unsafe {
        sys::fprintf(
            sys::stderr,
            b"bzip2, a block-sorting file compressor.  Version %s.\n   \n   Copyright (C) 1996-2019 by Julian Seward.\n   \n   This program is free software; you can redistribute it and/or modify\n   it under the terms set out in the LICENSE file, which is included\n   in the bzip2 source distribution.\n   \n   This program is distributed in the hope that it will be useful,\n   but WITHOUT ANY WARRANTY; without even the implied warranty of\n   MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the\n   LICENSE file for more details.\n   \n\0"
                .as_ptr() as *const c_char,
            BZ2_bzlibVersion(),
        );
    }
}

fn usage(full_prog_name: &[u8]) {
    let cf = CString::new(full_prog_name).unwrap_or_else(|_| CString::new("bzip2").unwrap());
    unsafe {
        sys::fprintf(
            sys::stderr,
            b"bzip2, a block-sorting file compressor.  Version %s.\n\n   usage: %s [flags and input files in any order]\n\n   -h --help           print this message\n   -d --decompress     force decompression\n   -z --compress       force compression\n   -k --keep           keep (don't delete) input files\n   -f --force          overwrite existing output files\n   -t --test           test compressed file integrity\n   -c --stdout         output to standard out\n   -q --quiet          suppress noncritical error messages\n   -v --verbose        be verbose (a 2nd -v gives more)\n   -L --license        display software version & license\n   -V --version        display software version & license\n   -s --small          use less memory (at most 2500k)\n   -1 .. -9            set block size to 100k .. 900k\n   --fast              alias for -1\n   --best              alias for -9\n\n   If invoked as `bzip2', default action is to compress.\n              as `bunzip2',  default action is to decompress.\n              as `bzcat', default action is to decompress to stdout.\n\n   If no file names are given, bzip2 compresses or decompresses\n   from standard input to standard output.  You can combine\n   short flags, so `-v -4' means the same as -v4 or -4v, &c.\n\n\0"
                .as_ptr() as *const c_char,
            BZ2_bzlibVersion(),
            cf.as_ptr(),
        );
    }
}

fn redundant(flag: &[u8]) {
    let cf = CString::new(flag).unwrap_or_else(|_| CString::new("?").unwrap());
    unsafe {
        sys::fprintf(
            sys::stderr,
            b"%s: %s is redundant in versions 0.9.5 and above\n\0".as_ptr() as *const c_char,
            prog_name(),
            cf.as_ptr(),
        );
    }
}

/*---------------------------------------------------*/
/*--- Command-line argument list                  ---*/
/*---------------------------------------------------*/

fn addFlagsFromEnvVar(arg_list: &mut Vec<Vec<u8>>, var_name: &str) {
    if let Some(envbase) = std::env::var_os(var_name) {
        let bytes = envbase.as_bytes().to_vec();
        let mut i = 0usize;
        let mut p = 0usize;
        loop {
            if p + i >= bytes.len() || bytes[p + i] == 0 {
                /* C scans NUL-terminated; OsString has no NULs, so
                   stop at end */
                if p + i >= bytes.len() {
                    break;
                }
                break;
            }
            p += i;
            i = 0;
            while p < bytes.len() && sys::c_isspace(bytes[p]) {
                p += 1;
            }
            while p + i < bytes.len() && bytes[p + i] != 0 && !sys::c_isspace(bytes[p + i]) {
                i += 1;
            }
            if i > 0 {
                let k = i.min(FILE_NAME_LEN - 10);
                let tok = bytes[p..p + k].to_vec();
                arg_list.push(tok); /* APPEND_FLAG */
            }
        }
    }
}

fn main() {
    unsafe {
        /*-- Be really really really paranoid :-) --*/
        if std::mem::size_of::<i32>() != 4
            || std::mem::size_of::<u32>() != 4
            || std::mem::size_of::<i16>() != 2
            || std::mem::size_of::<u16>() != 2
            || std::mem::size_of::<c_char>() != 1
        {
            configError();
        }

        /*-- Initialise --*/
        g().output_handle_just_in_case = std::ptr::null_mut();
        g().small_mode = FALSE;
        g().keep_input_files = FALSE;
        g().force_overwrite = FALSE;
        g().noisy = TRUE;
        g().verbosity = 0;
        g().block_size_100k = 9;
        g().test_fails_exist = FALSE;
        g().unz_fails_exist = FALSE;
        g().num_file_names = 0;
        g().num_files_processed = 0;
        g().work_factor = 30;
        g().delete_output_on_interrupt = FALSE;
        g().exit_value = 0;

        /*-- Set up signal handlers for mem access errors --*/
        sys::signal(sys::SIGSEGV, mySIGSEGVorSIGBUScatcher as *const () as usize);
        sys::signal(sys::SIGBUS, mySIGSEGVorSIGBUScatcher as *const () as usize);

        copyFileName(g().in_name.as_mut_ptr(), b"(none)");
        copyFileName(g().out_name.as_mut_ptr(), b"(none)");

        let argv: Vec<Vec<u8>> = std::env::args_os()
            .map(|a| a.as_bytes().to_vec())
            .collect();
        copyFileName(g().prog_name_really.as_mut_ptr(), &argv[0]);
        /* progName = tail past last '/' */
        let prl = c_bytes(g().prog_name_really.as_ptr());
        let tail = match prl.iter().rposition(|&c| c == b'/') {
            Some(p) => &prl[p + 1..],
            None => &prl[..],
        };
        PROG_NAME = tail.to_vec();
        PROG_NAME_C = tail.to_vec();
        PROG_NAME_C.push(0);

        /*-- Copy flags from env var BZIP2, and expand filename
             wildcards in arg list. --*/
        let mut arg_list: Vec<Vec<u8>> = Vec::new();
        addFlagsFromEnvVar(&mut arg_list, "BZIP2");
        addFlagsFromEnvVar(&mut arg_list, "BZIP");
        for a in argv.iter().skip(1) {
            arg_list.push(a.clone()); /* APPEND_FILESPEC */
        }

        /*-- Find the length of the longest filename --*/
        g().longest_file_name = 7;
        g().num_file_names = 0;
        let mut decode = true;
        for name in arg_list.iter() {
            if c_eq_flag(name, b"--") {
                decode = false;
                continue;
            }
            if name.first() == Some(&b'-') && decode {
                continue;
            }
            g().num_file_names += 1;
            if g().longest_file_name < name.len() as i32 {
                g().longest_file_name = name.len() as i32;
            }
        }

        /*-- Determine source modes; flag handling may change this too. --*/
        if g().num_file_names == 0 {
            g().src_mode = SM_I2O;
        } else {
            g().src_mode = SM_F2F;
        }

        /*-- Determine what to do (compress/uncompress/test/cat). --*/
        g().op_mode = OM_Z;

        let pn = prog_name_bytes();
        if contains(pn, b"unzip") || contains(pn, b"UNZIP") {
            g().op_mode = OM_UNZ;
        }
        if contains(pn, b"z2cat")
            || contains(pn, b"Z2CAT")
            || contains(pn, b"zcat")
            || contains(pn, b"ZCAT")
        {
            g().op_mode = OM_UNZ;
            g().src_mode = if g().num_file_names == 0 { SM_I2O } else { SM_F2O };
        }

        /*-- Look at the flags. --*/
        for name in arg_list.iter() {
            if c_eq_flag(name, b"--") {
                break;
            }
            if name.first() == Some(&b'-') && name.get(1) != Some(&b'-') {
                for &ch in name.iter().skip(1) {
                    match ch {
                        b'c' => g().src_mode = SM_F2O,
                        b'd' => g().op_mode = OM_UNZ,
                        b'z' => g().op_mode = OM_Z,
                        b'f' => g().force_overwrite = TRUE,
                        b't' => g().op_mode = OM_TEST,
                        b'k' => g().keep_input_files = TRUE,
                        b's' => g().small_mode = TRUE,
                        b'q' => g().noisy = FALSE,
                        b'1'..=b'9' => g().block_size_100k = (ch - b'0') as i32,
                        b'V' | b'L' => license(),
                        b'v' => g().verbosity += 1,
                        b'h' => {
                            usage(prog_name_bytes());
                            sys::exit(0);
                        }
                        _ => {
                            let cf = CString::new(name.clone())
                                .unwrap_or_else(|_| CString::new("?").unwrap());
                            sys::fprintf(
                                sys::stderr,
                                b"%s: Bad flag `%s'\n\0".as_ptr() as *const c_char,
                                prog_name(),
                                cf.as_ptr(),
                            );
                            usage(prog_name_bytes());
                            sys::exit(1);
                        }
                    }
                }
            }
        }

        /*-- And again ... --*/
        for name in arg_list.iter() {
            if c_eq_flag(name, b"--") {
                break;
            }
            if c_eq_flag(name, b"--stdout") {
                g().src_mode = SM_F2O;
            } else if c_eq_flag(name, b"--decompress") {
                g().op_mode = OM_UNZ;
            } else if c_eq_flag(name, b"--compress") {
                g().op_mode = OM_Z;
            } else if c_eq_flag(name, b"--force") {
                g().force_overwrite = TRUE;
            } else if c_eq_flag(name, b"--test") {
                g().op_mode = OM_TEST;
            } else if c_eq_flag(name, b"--keep") {
                g().keep_input_files = TRUE;
            } else if c_eq_flag(name, b"--small") {
                g().small_mode = TRUE;
            } else if c_eq_flag(name, b"--quiet") {
                g().noisy = FALSE;
            } else if c_eq_flag(name, b"--version") {
                license();
            } else if c_eq_flag(name, b"--license") {
                license();
            } else if c_eq_flag(name, b"--exponential") {
                g().work_factor = 1;
            } else if c_eq_flag(name, b"--repetitive-best") {
                redundant(name);
            } else if c_eq_flag(name, b"--repetitive-fast") {
                redundant(name);
            } else if c_eq_flag(name, b"--fast") {
                g().block_size_100k = 1;
            } else if c_eq_flag(name, b"--best") {
                g().block_size_100k = 9;
            } else if c_eq_flag(name, b"--verbose") {
                g().verbosity += 1;
            } else if c_eq_flag(name, b"--help") {
                usage(prog_name_bytes());
                sys::exit(0);
            } else if name.starts_with(b"--") {
                let cf = CString::new(name.clone()).unwrap_or_else(|_| CString::new("?").unwrap());
                sys::fprintf(
                    sys::stderr,
                    b"%s: Bad flag `%s'\n\0".as_ptr() as *const c_char,
                    prog_name(),
                    cf.as_ptr(),
                );
                usage(prog_name_bytes());
                sys::exit(1);
            }
        }

        if g().verbosity > 4 {
            g().verbosity = 4;
        }
        if g().op_mode == OM_Z && g().small_mode != 0 && g().block_size_100k > 2 {
            g().block_size_100k = 2;
        }

        if g().op_mode == OM_TEST && g().src_mode == SM_F2O {
            fmt_1s("%s: -c and -t cannot be used together.\n", prog_name_bytes());
            sys::exit(1);
        }

        if g().src_mode == SM_F2O && g().num_file_names == 0 {
            g().src_mode = SM_I2O;
        }

        if g().op_mode != OM_Z {
            g().block_size_100k = 0;
        }

        if g().src_mode == SM_F2F {
            sys::signal(sys::SIGINT, mySignalCatcher as *const () as usize);
            sys::signal(sys::SIGTERM, mySignalCatcher as *const () as usize);
            sys::signal(sys::SIGHUP, mySignalCatcher as *const () as usize);
        }

        if g().op_mode == OM_Z {
            if g().src_mode == SM_I2O {
                compress(None);
            } else {
                let mut decode = true;
                for name in arg_list.iter() {
                    if c_eq_flag(name, b"--") {
                        decode = false;
                        continue;
                    }
                    if name.first() == Some(&b'-') && decode {
                        continue;
                    }
                    g().num_files_processed += 1;
                    compress(Some(name));
                }
            }
        } else if g().op_mode == OM_UNZ {
            g().unz_fails_exist = FALSE;
            if g().src_mode == SM_I2O {
                uncompress(None);
            } else {
                let mut decode = true;
                for name in arg_list.iter() {
                    if c_eq_flag(name, b"--") {
                        decode = false;
                        continue;
                    }
                    if name.first() == Some(&b'-') && decode {
                        continue;
                    }
                    g().num_files_processed += 1;
                    uncompress(Some(name));
                }
            }
            if g().unz_fails_exist != 0 {
                setExit(2);
                sys::exit(g().exit_value);
            }
        } else {
            g().test_fails_exist = FALSE;
            if g().src_mode == SM_I2O {
                testf(None);
            } else {
                let mut decode = true;
                for name in arg_list.iter() {
                    if c_eq_flag(name, b"--") {
                        decode = false;
                        continue;
                    }
                    if name.first() == Some(&b'-') && decode {
                        continue;
                    }
                    g().num_files_processed += 1;
                    testf(Some(name));
                }
            }
            if g().test_fails_exist != 0 {
                if g().noisy != 0 {
                    sys::fprintf(
                        sys::stderr,
                        b"\nYou can use the `bzip2recover' program to attempt to recover\ndata from undamaged sections of corrupted files.\n\n\0"
                            .as_ptr() as *const c_char,
                    );
                }
                setExit(2);
                sys::exit(g().exit_value);
            }
        }

        sys::exit(g().exit_value);
    }
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() {
        return true;
    }
    hay.windows(needle.len()).any(|w| w == needle)
}
