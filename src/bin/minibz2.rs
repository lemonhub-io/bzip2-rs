/*-----------------------------------------------------------*/
/*--- minibz2 -- libbz2 test program (dlltest.c)            ---*/
/*---                       ported to Rust, same behavior   ---*/
/*-----------------------------------------------------------*/

/*   minibz2
        libbz2.dll test program.
        by Yoshioka Tsuneo (tsuneo@rr.iij4u.or.jp)
        This file is Public Domain.  Welcome any email to me.

        usage: minibz2 [-d] [-{1,2,..9}] [[srcfilename] destfilename]
*/

#![allow(clippy::manual_c_str_literals)]
#![allow(clippy::unnecessary_cast)]
#![allow(clippy::collapsible_if)]
use bz2::sys;
use bz2::*;
use std::ffi::CString;
use std::os::raw::{c_char, c_void};
use std::os::unix::ffi::OsStrExt;

fn usage() {
    unsafe {
        sys::fputs(
            b"usage: minibz2 [-d] [-{1,2,..9}] [[srcfilename] destfilename]\n\0".as_ptr()
                as *const c_char,
            sys::stdout,
        );
    }
}

fn main() {
    unsafe {
        let argv: Vec<Vec<u8>> = std::env::args_os()
            .map(|a| a.as_bytes().to_vec())
            .collect();

        let mut decompress = 0i32;
        let mut level: i32 = 9;
        let mut fn_r: Option<Vec<u8>> = None;
        let mut fn_w: Option<Vec<u8>> = None;

        let mut idx = 1usize;
        while idx < argv.len() {
            let arg = &argv[idx];
            if arg.first() == Some(&b'-') || arg.first() == Some(&b'/') {
                for &ch in arg.iter().skip(1) {
                    if ch == b'd' {
                        decompress = 1;
                    } else if (b'1'..=b'9').contains(&ch) {
                        level = (ch - b'0') as i32;
                    } else {
                        usage();
                        std::process::exit(1);
                    }
                }
                idx += 1;
            } else {
                break;
            }
        }
        if idx < argv.len() {
            fn_r = Some(argv[idx].clone());
            idx += 1;
        }
        if idx < argv.len() {
            fn_w = Some(argv[idx].clone());
        }

        let mut buff = [0u8; 0x1000];
        let mut mode = [0u8; 10];

        if decompress != 0 {
            let bz2fp_r: *mut BZFILE;
            let fp_w: *mut sys::FILE;

            if let Some(w) = &fn_w {
                let cw = CString::new(w.clone()).unwrap();
                fp_w = sys::fopen(cw.as_ptr(), b"wb\0".as_ptr() as *const c_char);
                if fp_w.is_null() {
                    sys::fprintf(
                        sys::stdout,
                        b"can't open [%s]\n\0".as_ptr() as *const c_char,
                        cw.as_ptr(),
                    );
                    sys::perror(b"reason:\0".as_ptr() as *const c_char);
                    std::process::exit(1);
                }
            } else {
                fp_w = sys::stdout;
            }
            let r_ok = if fn_r.is_none() {
                bz2fp_r = BZ2_bzdopen(sys::fileno(sys::stdin), b"rb\0".as_ptr() as *const c_char);
                !bz2fp_r.is_null()
            } else {
                let cr = CString::new(fn_r.clone().unwrap()).unwrap();
                bz2fp_r = BZ2_bzopen(cr.as_ptr(), b"rb\0".as_ptr() as *const c_char);
                !bz2fp_r.is_null()
            };
            if !r_ok {
                sys::fprintf(
                    sys::stdout,
                    b"can't bz2openstream\n\0".as_ptr() as *const c_char,
                );
                std::process::exit(1);
            }
            loop {
                let len = BZ2_bzread(bz2fp_r, buff.as_mut_ptr() as *mut c_void, 0x1000);
                if len <= 0 {
                    break;
                }
                sys::fwrite(buff.as_ptr() as *const c_void, 1, len as usize, fp_w);
            }
            BZ2_bzclose(bz2fp_r);
            if fp_w != sys::stdout {
                sys::fclose(fp_w);
            }
        } else {
            let bz2fp_w: *mut BZFILE;
            let fp_r: *mut sys::FILE;

            if let Some(r) = &fn_r {
                let cr = CString::new(r.clone()).unwrap();
                fp_r = sys::fopen(cr.as_ptr(), b"rb\0".as_ptr() as *const c_char);
                if fp_r.is_null() {
                    sys::fprintf(
                        sys::stdout,
                        b"can't open [%s]\n\0".as_ptr() as *const c_char,
                        cr.as_ptr(),
                    );
                    sys::perror(b"reason:\0".as_ptr() as *const c_char);
                    std::process::exit(1);
                }
            } else {
                fp_r = sys::stdin;
            }
            mode[0] = b'w';
            mode[1] = b'0' + level as u8;
            mode[2] = 0;

            let w_ok = if fn_w.is_none() {
                bz2fp_w = BZ2_bzdopen(sys::fileno(sys::stdout), mode.as_ptr() as *const c_char);
                !bz2fp_w.is_null()
            } else {
                let cw = CString::new(fn_w.clone().unwrap()).unwrap();
                bz2fp_w = BZ2_bzopen(cw.as_ptr(), mode.as_ptr() as *const c_char);
                !bz2fp_w.is_null()
            };
            if !w_ok {
                sys::fprintf(
                    sys::stdout,
                    b"can't bz2openstream\n\0".as_ptr() as *const c_char,
                );
                std::process::exit(1);
            }
            loop {
                let len = sys::fread(buff.as_mut_ptr() as *mut c_void, 1, 0x1000, fp_r) as i32;
                if len <= 0 {
                    break;
                }
                BZ2_bzwrite(bz2fp_w, buff.as_mut_ptr() as *mut c_void, len);
            }
            BZ2_bzclose(bz2fp_w);
            if fp_r != sys::stdin {
                sys::fclose(fp_r);
            }
        }
    }
}
