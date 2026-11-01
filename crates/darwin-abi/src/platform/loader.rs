use super::{bytes, errno_set, hooks, missing, stderr, stdin, stdout};
#[cfg(feature = "embedded")]
use super::{
    die, environ,
    image::{BINDINGS, ENTRY, INITIALIZERS, SEGMENTS},
    os::rune,
    profile, trace, unwind, EXECUTABLE, MACH_TASK, RUNELOCALE,
};
use libc::*;
use std::ffi::{CStr, CString};
#[cfg(feature = "embedded")]
use std::mem::zeroed;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

std::arch::global_asm!(".text", ".global darwin_chkstk", "darwin_chkstk:", "ret");
unsafe extern "C" {
    pub(super) fn darwin_chkstk();
}

#[repr(C)]
struct StaticImport {
    name: *const c_char,
    addr: *mut c_void,
}

unsafe extern "C" {
    static static_imports: StaticImport;
    static static_import_count: usize;
}

unsafe fn static_lookup(s: &CStr) -> *mut c_void {
    let imports =
        std::slice::from_raw_parts(std::ptr::addr_of!(static_imports), static_import_count);
    imports
        .binary_search_by(|i| CStr::from_ptr(i.name).to_bytes().cmp(s.to_bytes()))
        .map_or(null_mut(), |i| imports[i].addr)
}

unsafe extern "C" fn lazy_binding() -> ! {
    missing("unexpected lazy binding after eager relocation")
}

unsafe fn resolve(sym: &CStr) -> (*mut c_void, &'static str) {
    if sym.to_bytes() == b"dyld_stub_binder" {
        return (lazy_binding as *const () as *mut c_void, "loader-internal");
    }
    let name = sym.to_bytes().strip_prefix(b"_").unwrap_or(sym.to_bytes());
    if let Some(p) = hooks::resolve(name) {
        return (p, "adapter");
    }
    let p: *mut c_void = match name {
        b"__stdoutp" => std::ptr::addr_of_mut!(stdout).cast(),
        b"__stderrp" => std::ptr::addr_of_mut!(stderr).cast(),
        b"__stdinp" => std::ptr::addr_of_mut!(stdin).cast(),
        _ => null_mut(),
    };
    if !p.is_null() {
        return (p, "adapter");
    }
    let c = CString::new(name).unwrap();
    let p = static_lookup(&c);
    (p, if p.is_null() { "missing" } else { "static" })
}

unsafe extern "C" fn crash(sig: i32, info: *mut siginfo_t, ctx: *mut c_void) {
    let u = &*(ctx as *const ucontext_t);
    dprintf(
        STDERR_FILENO,
        c"FAULT %d address=%p rip=%llx rsp=%llx rbp=%llx\n".as_ptr(),
        sig,
        (*info).si_addr(),
        u.uc_mcontext.gregs[REG_RIP as usize],
        u.uc_mcontext.gregs[REG_RSP as usize],
        u.uc_mcontext.gregs[REG_RBP as usize],
    );
    signal(sig, SIG_DFL);
    raise(sig);
}

unsafe extern "C" {
    fn dprintf(fd: i32, fmt: *const c_char, ...) -> i32;
    fn __real_fwrite(p: *const c_void, size: usize, count: usize, f: *mut FILE) -> usize;
}

const PORT_LINE: &[u8] = concat!(
    "Built with ",
    env!("DTHD_BUILDER_NAME"),
    " ",
    env!("DTHD_PORT_VERSION"),
    " (",
    env!("DTHD_PORT_DATE"),
    ")\n"
)
.as_bytes();
const MARKER: &[u8] = b"Dolby Encoding Engine version ";
static PORT_DONE: AtomicBool = AtomicBool::new(false);
static PORT_STATE: Mutex<(usize, bool)> = Mutex::new((0, false));

#[no_mangle]
pub(super) unsafe extern "C" fn __wrap_fwrite(
    data: *const c_void,
    size: usize,
    count: usize,
    f: *mut FILE,
) -> usize {
    if f != stdout || size == 0 || count == 0 || PORT_DONE.load(Ordering::Acquire) {
        return __real_fwrite(data, size, count, f);
    }

    let mut state = PORT_STATE.lock().unwrap();
    if PORT_DONE.load(Ordering::Acquire) {
        return __real_fwrite(data, size, count, f);
    }

    let Some(n) = size.checked_mul(count) else {
        errno_set(EOVERFLOW);
        return 0;
    };

    let b = bytes(data.cast(), n);
    let mut cut = n;
    let mut done = false;
    for (i, &c) in b.iter().enumerate() {
        if state.1 && c == b'\n' {
            cut = i + 1;
            done = true;
            break;
        } else if !state.1 {
            state.0 = if c == MARKER[state.0] {
                state.0 + 1
            } else {
                usize::from(c == MARKER[0])
            };
            if state.0 == MARKER.len() {
                state.1 = true;
            }
        }
    }

    let mut wrote = __real_fwrite(data, 1, cut, f);
    if done && wrote == cut {
        __real_fwrite(PORT_LINE.as_ptr().cast(), 1, PORT_LINE.len(), f);
        if cut < n {
            wrote += __real_fwrite(data.cast::<u8>().add(cut).cast(), 1, n - cut, f);
        }
    }

    PORT_DONE.store(done, Ordering::Release);
    wrote / size
}

#[cfg(feature = "embedded")]
#[no_mangle]
pub(super) unsafe extern "C" fn main(argc: i32, argv: *mut *mut c_char) -> i32 {
    if realpath(
        c"/proc/self/exe".as_ptr(),
        std::ptr::addr_of_mut!(EXECUTABLE).cast(),
    )
    .is_null()
    {
        die("resolve executable");
    }
    if SEGMENTS.is_empty() || std::ptr::read_volatile(SEGMENTS[0].addr as *const u32) != 0xfeedfacf
    {
        eprintln!("embedded Mach-O image missing");
        return 125;
    }

    let mut sa: sigaction = zeroed();
    sa.sa_sigaction = crash as *const () as usize;
    sa.sa_flags = SA_SIGINFO;
    for sig in [SIGSEGV, SIGILL, SIGBUS] {
        sigaction(sig, &sa, null_mut());
    }

    let rune_ptr = std::ptr::addr_of_mut!(RUNELOCALE).cast::<u8>();
    for i in 0..256 {
        *rune_ptr.add(60).cast::<u32>().add(i) = rune(i as i32);
        *rune_ptr.add(1084).cast::<i32>().add(i) = tolower(i as i32);
        *rune_ptr.add(2108).cast::<i32>().add(i) = toupper(i as i32);
    }
    MACH_TASK = getpid() as u32;

    if std::env::var_os("MACHO_AUDIT").is_some() {
        println!("symbol\tresolution\tprovider");
        let mut seen = std::collections::HashSet::new();
        let mut missing = 0;
        for b in BINDINGS {
            if !seen.insert(b.name) {
                continue;
            }

            let (p, kind) = resolve(b.name);
            let mut info: Dl_info = zeroed();
            dladdr(p, &mut info);
            let provider = if info.dli_fname.is_null() {
                "launcher".into()
            } else {
                CStr::from_ptr(info.dli_fname).to_string_lossy()
            };
            println!("{}\t{kind}\t{provider}", b.name.to_string_lossy());
            if p.is_null() {
                missing += 1;
            }
        }

        return if missing == 0 { 0 } else { 125 };
    }

    let mut unresolved = 0;
    for b in BINDINGS {
        let (p, _) = resolve(b.name);
        if b.kind != 1 {
            eprintln!("unsupported bind type");
            return 125;
        }
        if p.is_null() {
            eprintln!("unresolved: {}", b.name.to_string_lossy());
            unresolved += 1;
        }
        std::ptr::write_unaligned(
            b.addr as *mut usize,
            (p as usize).wrapping_add(b.addend as usize),
        );
    }
    if unresolved != 0 {
        return 125;
    }

    for s in SEGMENTS {
        if mprotect(s.addr as *mut c_void, s.size, s.prot) != 0 {
            die("protect image");
        }
    }
    unwind::register();
    profile::start();

    let mut apple = [
        std::ptr::addr_of_mut!(EXECUTABLE).cast::<c_char>(),
        null_mut(),
    ];
    type Initializer =
        unsafe extern "C-unwind" fn(i32, *mut *mut c_char, *mut *mut c_char, *mut *mut c_char);
    for &address in INITIALIZERS {
        if trace() {
            eprintln!("init: {address:x}");
        }
        let f: Initializer = std::mem::transmute(address);
        f(argc, argv, environ, apple.as_mut_ptr());
    }

    if trace() {
        eprintln!("main: {ENTRY:x}");
    }
    let entry: unsafe extern "C-unwind" fn(
        i32,
        *mut *mut c_char,
        *mut *mut c_char,
        *mut *mut c_char,
    ) -> i32 = std::mem::transmute(ENTRY);
    entry(argc, argv, environ, apple.as_mut_ptr())
}
