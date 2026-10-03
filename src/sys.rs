/*-------------------------------------------------------------*/
/*--- libc bindings (the same calls the C code makes)       ---*/
/*---                                                sys.rs ---*/
/*-------------------------------------------------------------*/

/* This file is part of the Rust port of bzip2/libbzip2.

   Rather than reimplement buffered stdio / signals / uids, the port
   binds the identical libc entry points the original C code uses,
   preserving exact runtime behavior (buffering, fclose semantics,
   signal dispositions, errno-based diagnostics) with zero external
   crates.  Only the Unix path is provided, mirroring the C code's
   BZ_UNIX configuration. */

#![allow(non_camel_case_types)]

use std::os::raw::{c_char, c_int, c_uint, c_void};

pub enum FILE {}
pub type size_t = usize;

extern "C" {
    pub static stdin: *mut FILE;
    pub static stdout: *mut FILE;
    pub static stderr: *mut FILE;

    pub fn fopen(path: *const c_char, mode: *const c_char) -> *mut FILE;
    pub fn fdopen(fd: c_int, mode: *const c_char) -> *mut FILE;
    pub fn fclose(stream: *mut FILE) -> c_int;
    pub fn fread(ptr: *mut c_void, size: size_t, nmemb: size_t, stream: *mut FILE) -> size_t;
    pub fn fwrite(ptr: *const c_void, size: size_t, nmemb: size_t, stream: *mut FILE) -> size_t;
    pub fn fgetc(stream: *mut FILE) -> c_int;
    pub fn ungetc(c: c_int, stream: *mut FILE) -> c_int;
    pub fn ferror(stream: *mut FILE) -> c_int;
    pub fn fflush(stream: *mut FILE) -> c_int;
    pub fn rewind(stream: *mut FILE);
    pub fn fileno(stream: *mut FILE) -> c_int;
    pub fn fprintf(stream: *mut FILE, fmt: *const c_char, ...) -> c_int;
    pub fn fputs(s: *const c_char, stream: *mut FILE) -> c_int;
    pub fn fputc(c: c_int, stream: *mut FILE) -> c_int;
    pub fn putc(c: c_int, stream: *mut FILE) -> c_int;
    pub fn getc(stream: *mut FILE) -> c_int;
    pub fn putchar(c: c_int) -> c_int;
    pub fn perror(s: *const c_char);

    pub fn open(path: *const c_char, oflag: c_int, mode: c_uint) -> c_int;
    pub fn close(fd: c_int) -> c_int;
    pub fn remove(path: *const c_char) -> c_int;
    pub fn isatty(fd: c_int) -> c_int;
    pub fn fchmod(fd: c_int, mode: c_uint) -> c_int;
    pub fn fchown(fd: c_int, owner: c_uint, group: c_uint) -> c_int;
    pub fn utime(path: *const c_char, times: *const utimbuf) -> c_int;

    pub fn strerror(errnum: c_int) -> *mut c_char;
    pub fn exit(status: c_int) -> !;
    pub fn signal(signum: c_int, handler: usize) -> usize;
    pub fn malloc(size: size_t) -> *mut c_void;
    pub fn free(ptr: *mut c_void);

    pub fn srandom(seed: c_uint);
    pub fn random() -> std::os::raw::c_long;
}

pub use std::os::raw::c_long;

#[repr(C)]
pub struct utimbuf {
    pub actime: i64,  /* time_t */
    pub modtime: i64, /* time_t */
}

pub const O_WRONLY: c_int = 0o1;
pub const O_CREAT: c_int = 0o100;
pub const O_EXCL: c_int = 0o200;
pub const S_IWUSR: c_uint = 0o200;
pub const S_IRUSR: c_uint = 0o400;

pub const SIGINT: c_int = 2;
pub const SIGTERM: c_int = 15;
pub const SIGHUP: c_int = 1;
pub const SIGSEGV: c_int = 11;
pub const SIGBUS: c_int = 7;

pub const EOF: c_int = -1;

/*-- errno capture: matches C's errno read immediately after failure --*/
pub fn errno_val() -> c_int {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

/*-- strerror(errno) as String, for "Can't open input file %s: %s." --*/
pub unsafe fn strerror_string(errnum: c_int) -> String {
    let p = strerror(errnum);
    if p.is_null() {
        return String::new();
    }
    let s = std::ffi::CStr::from_ptr(p);
    s.to_string_lossy().into_owned()
}

/*-- C isspace/isdigit under the C locale --*/
#[inline]
pub fn c_isspace(c: u8) -> bool {
    matches!(c, b' ' | 0x09..=0x0d)
}

#[inline]
pub fn c_isdigit(c: u8) -> bool {
    c.is_ascii_digit()
}
