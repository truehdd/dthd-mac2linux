use super::die;
use libc::*;
use std::mem::zeroed;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicUsize, Ordering};

const PROFILE_CAP: usize = 1 << 22;
static PROFILE_COUNT: AtomicUsize = AtomicUsize::new(0);
static mut PROFILE_DATA: *mut u64 = null_mut();
static mut PROFILE_PATH: *const c_char = null();

unsafe extern "C" fn profile_tick(_sig: i32, _info: *mut siginfo_t, ctx: *mut c_void) {
    let i = PROFILE_COUNT.fetch_add(1, Ordering::Relaxed);
    if i < PROFILE_CAP {
        *PROFILE_DATA.add(i) =
            (*(ctx as *const ucontext_t)).uc_mcontext.gregs[REG_RIP as usize] as u64;
    }
}

unsafe extern "C" {
    fn setitimer(which: i32, new: *const itimerval, old: *mut itimerval) -> i32;
}

extern "C" fn profile_dump() {
    unsafe {
        let off: itimerval = zeroed();
        setitimer(ITIMER_PROF, &off, null_mut());

        let f = fopen(PROFILE_PATH, c"w".as_ptr());
        if f.is_null() {
            return;
        }

        let n = PROFILE_COUNT.load(Ordering::Relaxed).min(PROFILE_CAP);
        fprintf(f, c"# samples %zu\n".as_ptr(), n);
        for i in 0..n {
            let a = *PROFILE_DATA.add(i);
            let mut info: Dl_info = zeroed();
            if (0x100000000..0x200000000).contains(&a) {
                fprintf(f, c"%llx\tmacho\t-\n".as_ptr(), a);
            } else if dladdr(a as *const c_void, &mut info) != 0 && !info.dli_fname.is_null() {
                fprintf(
                    f,
                    c"%llx\t%s\t%s\t%llx\n".as_ptr(),
                    a,
                    info.dli_fname,
                    if info.dli_sname.is_null() {
                        c"?".as_ptr()
                    } else {
                        info.dli_sname
                    },
                    a - info.dli_fbase as u64,
                );
            } else {
                fprintf(f, c"%llx\t?\t?\n".as_ptr(), a);
            }
        }

        fclose(f);
    }
}

pub(super) unsafe fn start() {
    PROFILE_PATH = getenv(c"MACHO_PROFILE".as_ptr());
    if PROFILE_PATH.is_null() {
        return;
    }

    PROFILE_DATA = mmap(
        null_mut(),
        PROFILE_CAP * 8,
        3,
        MAP_PRIVATE | MAP_ANONYMOUS | MAP_NORESERVE,
        -1,
        0,
    )
    .cast();
    if PROFILE_DATA.cast::<c_void>() == MAP_FAILED {
        die("profile buffer");
    }

    let mut sa: sigaction = zeroed();
    sa.sa_sigaction = profile_tick as *const () as usize;
    sa.sa_flags = SA_SIGINFO | SA_RESTART;
    sigaction(SIGPROF, &sa, null_mut());
    atexit(profile_dump);

    let t = itimerval {
        it_interval: timeval {
            tv_sec: 0,
            tv_usec: 1000,
        },
        it_value: timeval {
            tv_sec: 0,
            tv_usec: 1000,
        },
    };
    setitimer(ITIMER_PROF, &t, null_mut());
}
