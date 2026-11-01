//! Darwin API adapters for the embedded encoder.
#![cfg_attr(not(feature = "embedded"), allow(dead_code))]

use libc::{__errno_location, c_char, c_void, FILE};

mod hooks;
mod image;
mod loader;
mod os;
mod profile;
mod sync;
mod text;
mod unwind;

unsafe extern "C" {
    static mut stdout: *mut FILE;
    static mut stderr: *mut FILE;
    static mut stdin: *mut FILE;
    static mut environ: *mut *mut c_char;
    fn bzero(p: *mut c_void, n: usize);
    fn exp10(x: f64) -> f64;
    fn exp10f(x: f32) -> f32;
    #[link_name = "_ZNSt3__15ctypeIcE2idE"]
    static cxx_ctype_id: u8;
}

static mut EXECUTABLE: [c_char; 4096] = [0; 4096];
static mut RUNELOCALE: [u32; 1024] = [0; 1024];
static mut STACK_GUARD: u64 = 0x785142839aaf1100;
static mut MACH_TASK: u32 = 0;

unsafe fn errno_set(n: i32) {
    *__errno_location() = n;
}

unsafe fn errno_get() -> i32 {
    *__errno_location()
}

fn die(s: &str) -> ! {
    eprintln!("{s}: {}", std::io::Error::last_os_error());
    std::process::exit(125)
}

fn missing(s: &str) -> ! {
    eprintln!("UNIMPLEMENTED import: {s}");
    std::process::abort()
}

fn trace() -> bool {
    std::env::var_os("MACHO_TRACE").is_some()
}

unsafe fn bytes<'a>(p: *const u8, n: usize) -> &'a [u8] {
    if n == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(p, n)
    }
}

unsafe fn units<'a>(p: *const u16, n: usize) -> &'a [u16] {
    if n == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(p, n)
    }
}

unsafe extern "C-unwind" {
    #[link_name = "_ZNKSt3__16locale9use_facetERNS0_2idE"]
    fn cxx_use_facet(loc: *mut c_void, id: *mut c_void) -> *mut c_void;
}
