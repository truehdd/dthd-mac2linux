use super::image::SEGMENTS;
use super::{cxx_ctype_id, cxx_use_facet, errno_get, errno_set, EXECUTABLE, RUNELOCALE};
use libc::*;
use std::cell::UnsafeCell;
use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::mem::zeroed;
use std::ptr::{null, null_mut};
use std::sync::{Mutex, OnceLock};

pub(super) unsafe extern "C" fn timebase(p: *mut u32) -> i32 {
    *p = 1;
    *p.add(1) = 1;
    0
}

pub(super) extern "C" fn audio_hosttime(ns: u64) -> u64 {
    ns
}

pub(super) unsafe extern "C" fn errno_location() -> *mut i32 {
    __errno_location()
}

pub(super) unsafe extern "C" fn random_bytes(p: *mut c_void, n: usize) -> i32 {
    let mut at = 0;
    while at < n {
        let r = getrandom(p.cast::<u8>().add(at).cast(), n - at, 0);
        if r < 0 && errno_get() == EINTR {
            continue;
        }
        if r <= 0 {
            return -1;
        }
        at += r as usize;
    }
    0
}

pub(super) unsafe extern "C" fn executable_path(p: *mut c_char, n: *mut u32) -> i32 {
    let src = std::ptr::addr_of!(EXECUTABLE).cast::<c_char>();
    let len = strlen(src) + 1;
    if (*n as usize) < len {
        *n = len as u32;
        return -1;
    }

    memcpy(p.cast(), src.cast(), len);
    0
}

pub(super) unsafe extern "C" fn memset_pattern16(p: *mut u8, pattern: *const u8, n: usize) {
    for i in 0..n {
        *p.add(i) = *pattern.add(i % 16);
    }
}

#[repr(C)]
pub(super) struct DoublePair {
    pub s: f64,
    pub c: f64,
}

pub(super) extern "C" fn darwin_sincos(x: f64) -> DoublePair {
    DoublePair {
        s: x.sin(),
        c: x.cos(),
    }
}

#[repr(C)]
pub(super) struct DarwinStat {
    dev: i32,
    mode: u16,
    nlink: u16,
    ino: u64,
    uid: u32,
    gid: u32,
    rdev: i32,
    pad: i32,
    at: timespec,
    mt: timespec,
    ct: timespec,
    bt: timespec,
    size: i64,
    blocks: i64,
    blksize: i32,
    flags: u32,
    gen: u32,
    spare: i32,
    qspare: [i64; 2],
}

unsafe fn copy_stat(d: *mut DarwinStat, s: &stat) {
    *d = zeroed();
    (*d).dev = s.st_dev as i32;
    (*d).mode = s.st_mode as u16;
    (*d).nlink = s.st_nlink as u16;
    (*d).ino = s.st_ino;
    (*d).uid = s.st_uid;
    (*d).gid = s.st_gid;
    (*d).rdev = s.st_rdev as i32;
    (*d).at = timespec {
        tv_sec: s.st_atime,
        tv_nsec: s.st_atime_nsec,
    };
    (*d).mt = timespec {
        tv_sec: s.st_mtime,
        tv_nsec: s.st_mtime_nsec,
    };
    (*d).ct = timespec {
        tv_sec: s.st_ctime,
        tv_nsec: s.st_ctime_nsec,
    };
    (*d).size = s.st_size;
    (*d).blocks = s.st_blocks;
    (*d).blksize = s.st_blksize as i32;
}

pub(super) unsafe extern "C" fn darwin_stat(p: *const c_char, d: *mut DarwinStat) -> i32 {
    let mut s = zeroed();
    let r = stat(p, &mut s);
    if r == 0 {
        copy_stat(d, &s);
    }
    r
}

pub(super) unsafe extern "C" fn darwin_lstat(p: *const c_char, d: *mut DarwinStat) -> i32 {
    let mut s = zeroed();
    let r = lstat(p, &mut s);
    if r == 0 {
        copy_stat(d, &s);
    }
    r
}

pub(super) unsafe extern "C" fn darwin_fstat(fd: i32, d: *mut DarwinStat) -> i32 {
    let mut s = zeroed();
    let r = fstat(fd, &mut s);
    if r == 0 {
        copy_stat(d, &s);
    }
    r
}

pub(super) fn darwin_flags(f: i32) -> i32 {
    let mut r = f & 3;
    for (d, n) in [
        (8, O_APPEND),
        (0x200, O_CREAT),
        (0x400, O_TRUNC),
        (0x800, O_EXCL),
        (4, O_NONBLOCK),
        (0x1000000, O_CLOEXEC),
        (0x80, O_SYNC),
        (0x100, O_NOFOLLOW),
        (0x20000, O_NOCTTY),
        (0x100000, O_DIRECTORY),
        (0x400000, O_DSYNC),
    ] {
        if f & d != 0 {
            r |= n;
        }
    }
    r
}

fn native_flags(f: i32) -> i32 {
    let mut r = f & O_ACCMODE;
    for (d, n) in [
        (8, O_APPEND),
        (4, O_NONBLOCK),
        (0x200, O_CREAT),
        (0x400, O_TRUNC),
        (0x800, O_EXCL),
    ] {
        if f & n != 0 {
            r |= d;
        }
    }
    r
}

// x86-64 SysV places the optional integer argument in RDX in both ABIs. It is
// ignored for calls that do not supply it; no va_list representation is exposed.
pub(super) unsafe extern "C" fn darwin_open(p: *const c_char, f: i32, mode: u32) -> i32 {
    open(p, darwin_flags(f), if f & 0x200 != 0 { mode } else { 0 })
}

pub(super) unsafe extern "C" fn darwin_shm_open(p: *const c_char, f: i32, mode: u32) -> i32 {
    shm_open(p, darwin_flags(f), mode)
}

pub(super) unsafe extern "C" fn darwin_mmap(
    p: *mut c_void,
    n: usize,
    prot: i32,
    flags: i32,
    fd: i32,
    offset: i64,
) -> *mut c_void {
    let mut f = flags & 0x13;
    if flags & 0x1000 != 0 {
        f |= MAP_ANONYMOUS;
    }
    if flags & 0x40 != 0 {
        f |= MAP_NORESERVE;
    }
    mmap(p, n, prot, f, fd, offset)
}

#[repr(C)]
pub(super) struct DarwinDirent {
    ino: u64,
    seekoff: u64,
    reclen: u16,
    namlen: u16,
    kind: u8,
    name: [c_char; 1024],
}

thread_local! {
    static DIRENT: UnsafeCell<DarwinDirent> = const { UnsafeCell::new(unsafe { zeroed() }) };
    static LCONV: UnsafeCell<[u8; 96]> = const { UnsafeCell::new([0; 96]) };
}

pub(super) unsafe extern "C" fn darwin_readdir(dir: *mut DIR) -> *mut DarwinDirent {
    let d = readdir(dir);
    if d.is_null() {
        return null_mut();
    }

    DIRENT.with(|cell| {
        let out = cell.get();
        *out = zeroed();
        (*out).ino = (*d).d_ino;
        (*out).seekoff = (*d).d_off as u64;
        (*out).kind = (*d).d_type;

        let name = std::ptr::addr_of!((*d).d_name).cast::<c_char>();
        let n = strlen(name);
        (*out).namlen = n as u16;
        (*out).reclen = ((21 + n + 1 + 3) & !3) as u16;
        memcpy(
            std::ptr::addr_of_mut!((*out).name).cast(),
            name.cast(),
            n + 1,
        );
        out
    })
}

pub(super) unsafe extern "C" fn darwin_readdir_r(
    dir: *mut DIR,
    out: *mut DarwinDirent,
    result: *mut *mut DarwinDirent,
) -> i32 {
    errno_set(0);
    let d = darwin_readdir(dir);
    if d.is_null() {
        *result = null_mut();
        errno_get()
    } else {
        std::ptr::copy_nonoverlapping(d, out, 1);
        *result = out;
        0
    }
}

pub(super) unsafe extern "C" fn darwin_fgetpos(f: *mut FILE, o: *mut i64) -> i32 {
    *o = ftello(f);
    if *o < 0 {
        -1
    } else {
        0
    }
}

pub(super) unsafe extern "C" fn darwin_fsetpos(f: *mut FILE, o: *const i64) -> i32 {
    fseeko(f, *o, SEEK_SET)
}

static FILES: OnceLock<Mutex<HashMap<usize, bool>>> = OnceLock::new();

fn file_record(f: *mut FILE) -> *mut FILE {
    if !f.is_null() {
        FILES
            .get_or_init(Default::default)
            .lock()
            .unwrap()
            .insert(f as usize, false);
    }
    f
}

pub(super) unsafe extern "C" fn darwin_fopen(p: *const c_char, m: *const c_char) -> *mut FILE {
    file_record(fopen(p, m))
}

pub(super) unsafe extern "C" fn darwin_fdopen(fd: i32, m: *const c_char) -> *mut FILE {
    file_record(fdopen(fd, m))
}

pub(super) unsafe extern "C" fn darwin_fclose(f: *mut FILE) -> i32 {
    let mut g = FILES.get_or_init(Default::default).lock().unwrap();
    if let Some(closed) = g.get_mut(&(f as usize)) {
        if *closed {
            errno_set(EBADF);
            return EOF;
        }
        *closed = true;
    }

    fclose(f)
}

pub(super) unsafe extern "C" fn rune(c: i32) -> u32 {
    if !(0..=255).contains(&c) {
        return 0;
    }

    let mut r = 0;
    for (test, mask) in [
        (isalpha(c), 0x100),
        (iscntrl(c), 0x200),
        (isdigit(c), 0x400),
        (isgraph(c), 0x800),
        (islower(c), 0x1000),
        (ispunct(c), 0x2000),
        (isspace(c), 0x4000),
        (isupper(c), 0x8000),
        (isxdigit(c), 0x10000),
        (isblank(c), 0x20000),
        (isprint(c), 0x40000),
    ] {
        if test != 0 {
            r |= mask;
        }
    }
    r
}

pub(super) unsafe extern "C" fn maskrune(c: i32, mask: u32) -> u32 {
    rune(c) & mask
}

pub(super) unsafe extern "C" fn darwin_dlopen(name: *const c_char, _flags: i32) -> *mut c_void {
    let name = if !name.is_null()
        && CStr::from_ptr(name)
            .to_bytes()
            .windows(9)
            .any(|s| s == b"mediainfo")
    {
        c"libmediainfo.so.0".as_ptr()
    } else {
        name
    };
    dlopen(name, RTLD_LAZY | RTLD_LOCAL)
}

pub(super) unsafe extern "C" fn darwin_dlsym(
    handle: *mut c_void,
    name: *const c_char,
) -> *mut c_void {
    dlsym(
        if handle as isize == -2 {
            RTLD_DEFAULT
        } else {
            handle
        },
        name,
    )
}

pub(super) unsafe extern "C" fn darwin_dladdr(addr: *const c_void, info: *mut Dl_info) -> i32 {
    let a = addr as usize;
    if SEGMENTS.iter().any(|s| a >= s.addr && a < s.addr + s.size) {
        *info = zeroed();
        (*info).dli_fname = std::ptr::addr_of!(EXECUTABLE).cast();
        (*info).dli_fbase = 0x100000000usize as *mut c_void;
        1
    } else {
        dladdr(addr, info)
    }
}

pub(super) unsafe extern "C" fn system_now() -> i64 {
    let mut t = zeroed();
    clock_gettime(CLOCK_REALTIME, &mut t);
    t.tv_sec * 1_000_000 + t.tv_nsec / 1000
}

pub(super) unsafe extern "C" fn system_to_time(us: *const i64) -> i64 {
    *us / 1_000_000
}

static FACET_LOCK: Mutex<()> = Mutex::new(());

#[repr(C, align(8))]
struct Facet {
    bytes: [u8; 64],
}

static mut FACET: Facet = Facet { bytes: [0; 64] };

pub(super) unsafe extern "C-unwind" fn darwin_use_facet(
    loc: *mut c_void,
    id: *mut c_void,
) -> *mut c_void {
    let p = cxx_use_facet(loc, id);
    if id == std::ptr::addr_of!(cxx_ctype_id).cast_mut().cast() {
        let _guard = FACET_LOCK.lock().unwrap();
        let facet = std::ptr::addr_of_mut!(FACET.bytes).cast::<u8>();
        memcpy(facet.cast(), p, 32);
        *facet.add(16).cast::<*mut u8>() = std::ptr::addr_of_mut!(RUNELOCALE).cast::<u8>().add(60);
        facet.cast()
    } else {
        p
    }
}

pub(super) unsafe extern "C" fn darwin_setlocale(
    category: i32,
    locale: *const c_char,
) -> *mut c_char {
    let cats = [
        LC_ALL,
        LC_COLLATE,
        LC_CTYPE,
        LC_MONETARY,
        LC_NUMERIC,
        LC_TIME,
        LC_MESSAGES,
    ];
    if let Some(c) = cats.get(category as usize) {
        setlocale(*c, locale)
    } else {
        errno_set(EINVAL);
        null_mut()
    }
}

pub(super) unsafe extern "C" fn darwin_localeconv() -> *mut c_void {
    LCONV.with(|cell| {
        let n = localeconv();
        let out = cell.get().cast::<u8>();
        memcpy(out.cast(), n.cast(), 88);
        for (i, c) in [
            (*n).int_p_cs_precedes,
            (*n).int_n_cs_precedes,
            (*n).int_p_sep_by_space,
            (*n).int_n_sep_by_space,
            (*n).int_p_sign_posn,
            (*n).int_n_sign_posn,
        ]
        .iter()
        .enumerate()
        {
            *out.add(88 + i) = *c as u8;
        }
        out.cast()
    })
}

pub(super) unsafe extern "C" fn sysconf_full(n: i32) -> i64 {
    let n = match n {
        1 => _SC_ARG_MAX,
        2 => _SC_CHILD_MAX,
        3 => _SC_CLK_TCK,
        4 => _SC_NGROUPS_MAX,
        5 => _SC_OPEN_MAX,
        6 => _SC_JOB_CONTROL,
        7 => _SC_SAVED_IDS,
        8 => _SC_VERSION,
        29 => _SC_PAGESIZE,
        57 => _SC_NPROCESSORS_CONF,
        58 => _SC_NPROCESSORS_ONLN,
        _ => {
            errno_set(EINVAL);
            return -1;
        }
    };
    sysconf(n)
}

pub(super) unsafe extern "C" fn darwin_pathconf(p: *const c_char, n: i32) -> i64 {
    let names = [
        -1,
        _PC_LINK_MAX,
        _PC_MAX_CANON,
        _PC_MAX_INPUT,
        _PC_NAME_MAX,
        _PC_PATH_MAX,
        _PC_PIPE_BUF,
        _PC_CHOWN_RESTRICTED,
        _PC_NO_TRUNC,
        _PC_VDISABLE,
    ];
    if !(1..10).contains(&n) {
        errno_set(EINVAL);
        -1
    } else {
        pathconf(p, names[n as usize])
    }
}

pub(super) unsafe extern "C" fn sysctl_full(
    m: *const i32,
    n: u32,
    out: *mut c_void,
    len: *mut usize,
    input: *const c_void,
    _ilen: usize,
) -> i32 {
    if m.is_null() || len.is_null() || n != 2 || !input.is_null() {
        errno_set(EINVAL);
        return -1;
    }

    let mut u: utsname = zeroed();
    uname(&mut u);
    let data: Vec<u8> = match (*m, *m.add(1)) {
        (1, k) => {
            let p = match k {
                1 => u.sysname.as_ptr(),
                2 => u.release.as_ptr(),
                4 => u.version.as_ptr(),
                10 => u.nodename.as_ptr(),
                _ => {
                    errno_set(ENOENT);
                    return -1;
                }
            };
            CStr::from_ptr(p).to_bytes_with_nul().to_vec()
        }
        (6, 1 | 2 | 12) => CStr::from_ptr(u.machine.as_ptr())
            .to_bytes_with_nul()
            .to_vec(),
        (6, 3 | 25) => (sysconf(_SC_NPROCESSORS_ONLN) as u32)
            .to_ne_bytes()
            .to_vec(),
        (6, 4) => 1234u32.to_ne_bytes().to_vec(),
        (6, 7) => (sysconf(_SC_PAGESIZE) as u32).to_ne_bytes().to_vec(),
        (6, 24) => ((sysconf(_SC_PHYS_PAGES) as u64) * (sysconf(_SC_PAGESIZE) as u64))
            .to_ne_bytes()
            .to_vec(),
        _ => {
            errno_set(ENOENT);
            return -1;
        }
    };

    let capacity = *len;
    *len = data.len();
    if out.is_null() {
        return 0;
    }
    memcpy(out, data.as_ptr().cast(), capacity.min(data.len()));
    if capacity < data.len() {
        errno_set(ENOMEM);
        -1
    } else {
        0
    }
}

#[repr(C)]
struct DarwinFlock {
    start: i64,
    length: i64,
    pid: i32,
    kind: i16,
    whence: i16,
}

pub(super) unsafe extern "C" fn darwin_fcntl(fd: i32, cmd: i32, arg: isize) -> i32 {
    match cmd {
        0 => fcntl(fd, F_DUPFD, arg),
        1 => fcntl(fd, F_GETFD),
        2 => fcntl(fd, F_SETFD, arg),
        3 => {
            let r = fcntl(fd, F_GETFL);
            if r < 0 {
                r
            } else {
                native_flags(r)
            }
        }
        4 => fcntl(fd, F_SETFL, darwin_flags(arg as i32)),
        5 => fcntl(fd, F_GETOWN),
        6 => fcntl(fd, F_SETOWN, arg),
        67 => fcntl(fd, F_DUPFD_CLOEXEC, arg),
        7..=9 => {
            let d = &mut *(arg as *mut DarwinFlock);
            let mut n: flock = zeroed();
            n.l_start = d.start;
            n.l_len = d.length;
            n.l_pid = d.pid;
            n.l_whence = d.whence;
            n.l_type = match d.kind {
                1 => F_RDLCK,
                2 => F_UNLCK,
                _ => F_WRLCK,
            } as i16;
            let r = fcntl(
                fd,
                match cmd {
                    7 => F_GETLK,
                    8 => F_SETLK,
                    _ => F_SETLKW,
                },
                &mut n,
            );
            if r == 0 && cmd == 7 {
                d.start = n.l_start;
                d.length = n.l_len;
                d.pid = n.l_pid;
                d.whence = n.l_whence;
                d.kind = if n.l_type == F_RDLCK as i16 {
                    1
                } else if n.l_type == F_UNLCK as i16 {
                    2
                } else {
                    3
                };
            }
            r
        }
        50 | 102 => {
            let p = CString::new(format!("/proc/self/fd/{fd}")).unwrap();
            let n = readlink(p.as_ptr(), arg as *mut c_char, 1023);
            if n < 0 {
                -1
            } else {
                *(arg as *mut c_char).add(n as usize) = 0;
                0
            }
        }
        51 => fsync(fd),
        _ => {
            errno_set(EINVAL);
            -1
        }
    }
}

const INPUTFLAGS: &[(u64, tcflag_t)] = &[
    (1, IGNBRK),
    (2, BRKINT),
    (4, IGNPAR),
    (8, PARMRK),
    (16, INPCK),
    (32, ISTRIP),
    (64, INLCR),
    (128, IGNCR),
    (256, ICRNL),
    (512, IXON),
    (1024, IXOFF),
    (2048, IXANY),
    (8192, IMAXBEL),
    (16384, IUTF8),
];
const OUTPUTFLAGS: &[(u64, tcflag_t)] = &[
    (1, OPOST),
    (2, ONLCR),
    (16, OCRNL),
    (32, ONOCR),
    (64, ONLRET),
    (128, OFILL),
    (0x20000, OFDEL),
];
const CONTROLFLAGS: &[(u64, tcflag_t)] = &[
    (0x400, CSTOPB),
    (0x800, CREAD),
    (0x1000, PARENB),
    (0x2000, PARODD),
    (0x4000, HUPCL),
    (0x8000, CLOCAL),
    (0x30000, CRTSCTS),
];
const LOCALFLAGS: &[(u64, tcflag_t)] = &[
    (1, ECHOKE),
    (2, ECHOE),
    (4, ECHOK),
    (8, ECHO),
    (16, ECHONL),
    (32, ECHOPRT),
    (64, ECHOCTL),
    (128, ISIG),
    (256, ICANON),
    (1024, IEXTEN),
    (2048, EXTPROC),
    (0x400000, TOSTOP),
    (0x800000, FLUSHO),
    (0x20000000, PENDIN),
    (0x80000000, NOFLSH),
];
const CONTROLCHARS: [i32; 20] = [
    VEOF as i32,
    VEOL as i32,
    VEOL2 as i32,
    VERASE as i32,
    VWERASE as i32,
    VKILL as i32,
    VREPRINT as i32,
    -1,
    VINTR as i32,
    VQUIT as i32,
    VSUSP as i32,
    -1,
    VSTART as i32,
    VSTOP as i32,
    VLNEXT as i32,
    VDISCARD as i32,
    VMIN as i32,
    VTIME as i32,
    -1,
    -1,
];
const SPEEDS: &[(u64, speed_t)] = &[
    (0, B0),
    (50, B50),
    (75, B75),
    (110, B110),
    (134, B134),
    (150, B150),
    (200, B200),
    (300, B300),
    (600, B600),
    (1200, B1200),
    (1800, B1800),
    (2400, B2400),
    (4800, B4800),
    (9600, B9600),
    (19200, B19200),
    (38400, B38400),
    (57600, B57600),
    (115200, B115200),
    (230400, B230400),
];
#[repr(C)]
pub(super) struct DarwinTermios {
    input: u64,
    output: u64,
    control: u64,
    local: u64,
    cc: [u8; 20],
    ispeed: u64,
    ospeed: u64,
}

fn darwin_flags_to(input: tcflag_t, p: &[(u64, tcflag_t)]) -> u64 {
    p.iter()
        .filter(|(_, n)| input & n != 0)
        .fold(0, |v, (d, _)| v | d)
}

fn flags_to_n(input: u64, p: &[(u64, tcflag_t)]) -> tcflag_t {
    p.iter()
        .filter(|(d, _)| input & d != 0)
        .fold(0, |v, (_, n)| v | n)
}

fn darwin_speed(s: speed_t) -> u64 {
    SPEEDS.iter().find(|(_, n)| *n == s).map_or(0, |(d, _)| *d)
}

fn speed_n(s: u64) -> speed_t {
    SPEEDS.iter().find(|(d, _)| *d == s).map_or(B0, |(_, n)| *n)
}

pub(super) unsafe extern "C" fn darwin_tcgetattr(fd: i32, d: *mut DarwinTermios) -> i32 {
    let mut n: termios = zeroed();
    if tcgetattr(fd, &mut n) != 0 {
        return -1;
    }

    *d = zeroed();
    let d = &mut *d;
    d.input = darwin_flags_to(n.c_iflag, INPUTFLAGS);
    d.output = darwin_flags_to(n.c_oflag, OUTPUTFLAGS);
    d.control = darwin_flags_to(n.c_cflag, CONTROLFLAGS) | (((n.c_cflag & CSIZE) >> 4) << 8) as u64;
    d.local = darwin_flags_to(n.c_lflag, LOCALFLAGS);
    for (i, k) in CONTROLCHARS.iter().enumerate() {
        d.cc[i] = if *k >= 0 && n.c_cc[*k as usize] != 0 {
            n.c_cc[*k as usize]
        } else {
            255
        };
    }
    d.ispeed = darwin_speed(cfgetispeed(&n));
    d.ospeed = darwin_speed(cfgetospeed(&n));
    0
}

pub(super) unsafe extern "C" fn darwin_tcsetattr(
    fd: i32,
    action: i32,
    d: *const DarwinTermios,
) -> i32 {
    if !(0..=2).contains(&action) {
        errno_set(EINVAL);
        return -1;
    }
    let mut n: termios = zeroed();
    if tcgetattr(fd, &mut n) != 0 {
        return -1;
    }

    let d = &*d;
    n.c_iflag = flags_to_n(d.input, INPUTFLAGS);
    n.c_oflag = flags_to_n(d.output, OUTPUTFLAGS);
    n.c_cflag = flags_to_n(d.control, CONTROLFLAGS) | ((d.control & 0x300) >> 4) as u32;
    n.c_lflag = flags_to_n(d.local, LOCALFLAGS);
    for (i, k) in CONTROLCHARS.iter().enumerate() {
        if *k >= 0 {
            n.c_cc[*k as usize] = if d.cc[i] == 255 { 0 } else { d.cc[i] };
        }
    }

    cfsetispeed(&mut n, speed_n(d.ispeed));
    cfsetospeed(&mut n, speed_n(d.ospeed));
    tcsetattr(fd, action, &n)
}

const SIGNALS: [i32; 32] = [
    0, SIGHUP, SIGINT, SIGQUIT, SIGILL, SIGTRAP, SIGABRT, 0, SIGFPE, SIGKILL, SIGBUS, SIGSEGV,
    SIGSYS, SIGPIPE, SIGALRM, SIGTERM, SIGURG, SIGSTOP, SIGTSTP, SIGCONT, SIGCHLD, SIGTTIN,
    SIGTTOU, SIGIO, SIGXCPU, SIGXFSZ, SIGVTALRM, SIGPROF, SIGWINCH, 0, SIGUSR1, SIGUSR2,
];
#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct DarwinSigaction {
    handler: usize,
    mask: u32,
    flags: i32,
}

static mut ACTIONS: [DarwinSigaction; 32] = [DarwinSigaction {
    handler: 0,
    mask: 0,
    flags: 0,
}; 32];
#[repr(C)]
struct DarwinSiginfo {
    signo: i32,
    error: i32,
    code: i32,
    pid: i32,
    uid: u32,
    status: i32,
    addr: *mut c_void,
    value: sigval,
    band: i64,
    pad: [i64; 7],
}

unsafe extern "C" fn signal_bridge(sig: i32, info: *mut siginfo_t, ctx: *mut c_void) {
    let d = SIGNALS
        .iter()
        .position(|&s| s == sig)
        .unwrap_or(sig as usize);
    let a = ACTIONS[d];
    if a.flags & 0x40 != 0 {
        let info = &*info;
        let mut di = DarwinSiginfo {
            signo: d as i32,
            error: info.si_errno,
            code: info.si_code,
            pid: info.si_pid(),
            uid: info.si_uid(),
            status: info.si_status(),
            addr: info.si_addr(),
            value: info.si_value(),
            band: std::ptr::read_unaligned(
                (info as *const siginfo_t)
                    .cast::<u8>()
                    .add(16)
                    .cast::<i64>(),
            ),
            pad: [0; 7],
        };
        let f: unsafe extern "C" fn(i32, *mut DarwinSiginfo, *mut c_void) =
            std::mem::transmute(a.handler);
        f(d as i32, &mut di, ctx);
    } else {
        let f: unsafe extern "C" fn(i32) = std::mem::transmute(a.handler);
        f(d as i32);
    }
}

pub(super) unsafe extern "C" fn sigaction_d_fn(
    sig: i32,
    a: *const DarwinSigaction,
    old: *mut DarwinSigaction,
) -> i32 {
    if !(1..32).contains(&sig) || SIGNALS[sig as usize] == 0 {
        errno_set(EINVAL);
        return -1;
    }

    let sig = sig as usize;
    let saved = ACTIONS[sig];
    let mut sa: sigaction = zeroed();
    let mut prev: sigaction = zeroed();
    if !a.is_null() {
        sigemptyset(&mut sa.sa_mask);
        for (i, s) in SIGNALS.iter().enumerate().skip(1) {
            if (*a).mask & (1 << (i - 1)) != 0 && *s != 0 {
                sigaddset(&mut sa.sa_mask, *s);
            }
        }
        for (d, n) in [
            (1, SA_ONSTACK),
            (2, SA_RESTART),
            (4, SA_RESETHAND),
            (8, SA_NOCLDSTOP),
            (16, SA_NODEFER),
            (32, SA_NOCLDWAIT),
        ] {
            if (*a).flags & d != 0 {
                sa.sa_flags |= n;
            }
        }
        if (*a).handler == SIG_DFL || (*a).handler == SIG_IGN {
            sa.sa_sigaction = (*a).handler;
        } else {
            sa.sa_sigaction = signal_bridge as *const () as usize;
            sa.sa_flags |= SA_SIGINFO;
        }
        ACTIONS[sig] = *a;
    }

    let r = sigaction(
        SIGNALS[sig],
        if a.is_null() { null() } else { &sa },
        &mut prev,
    );
    if r != 0 {
        ACTIONS[sig] = saved;
        return r;
    }

    if !old.is_null() {
        *old = saved;
        if prev.sa_sigaction != signal_bridge as *const () as usize {
            (*old).handler = prev.sa_sigaction;
            (*old).mask = 0;
            (*old).flags = if prev.sa_flags & SA_RESTART != 0 {
                2
            } else {
                0
            };
            for (i, s) in SIGNALS.iter().enumerate().skip(1) {
                if *s != 0 && sigismember(&prev.sa_mask, *s) != 0 {
                    (*old).mask |= 1 << (i - 1);
                }
            }
        }
    }
    0
}

pub(super) unsafe extern "C" fn darwin_signal(sig: i32, handler: usize) -> usize {
    let a = DarwinSigaction {
        handler,
        mask: 0,
        flags: 2,
    };
    let mut old = zeroed();
    if sigaction_d_fn(sig, &a, &mut old) != 0 {
        SIG_ERR
    } else {
        old.handler
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::size_of;

    #[test]
    fn darwin_layouts() {
        assert_eq!(size_of::<DarwinStat>(), 144);
        assert_eq!(std::mem::offset_of!(DarwinDirent, name), 21);
        assert_eq!(size_of::<DarwinTermios>(), 72);
        assert_eq!(size_of::<DarwinSigaction>(), 16);
        assert_eq!(size_of::<lconv>(), 96);
    }

    #[test]
    fn file_handle_reuse_and_repeated_close() {
        unsafe {
            for _ in 0..20 {
                let f = darwin_fopen(c"/dev/null".as_ptr(), c"rb".as_ptr());
                assert!(!f.is_null());
                assert_eq!(darwin_fclose(f), 0);

                errno_set(0);
                assert_eq!(darwin_fclose(f), EOF);
                assert_eq!(errno_get(), EBADF);
            }
        }
    }

    #[test]
    fn sysctl_sizes_and_native_values() {
        unsafe {
            let mib = [6, 25];
            let mut cpus = 0u32;
            let mut len = 4;
            assert_eq!(
                sysctl_full(
                    mib.as_ptr(),
                    2,
                    (&mut cpus as *mut u32).cast(),
                    &mut len,
                    null(),
                    0
                ),
                0
            );
            assert_eq!(cpus, sysconf(_SC_NPROCESSORS_ONLN) as u32);

            len = 0;
            assert_eq!(
                sysctl_full(mib.as_ptr(), 2, null_mut(), &mut len, null(), 0),
                0
            );
            assert_eq!(len, 4);
            assert_eq!(sysconf_full(29), sysconf(_SC_PAGESIZE));

            let mut small = 0u8;
            len = 1;
            assert_eq!(
                sysctl_full(
                    mib.as_ptr(),
                    2,
                    (&mut small as *mut u8).cast(),
                    &mut len,
                    null(),
                    0
                ),
                -1
            );
            assert_eq!(errno_get(), ENOMEM);
            assert_eq!(len, 4);
        }
    }

    #[test]
    fn mapped_memory_and_file_flags() {
        unsafe {
            let p = darwin_mmap(null_mut(), 4096, 3, 0x1002, -1, 0);
            assert_ne!(p, MAP_FAILED);
            memset(p, 0xa5, 4096);
            assert_eq!(munmap(p, 4096), 0);

            let name = CString::new(format!("/dthd-rust-test-{}", getpid())).unwrap();
            let fd = darwin_shm_open(name.as_ptr(), 0xa02, 0o600);
            assert!(fd >= 0);
            assert_eq!(ftruncate(fd, 4096), 0);
            let p = darwin_mmap(null_mut(), 4096, 3, 1, fd, 0);
            assert_ne!(p, MAP_FAILED);
            memcpy(p, c"mapped".as_ptr().cast(), 7);
            munmap(p, 4096);
            close(fd);
            assert_eq!(shm_unlink(name.as_ptr()), 0);
        }
    }

    #[test]
    fn stat_and_directory_layout() {
        unsafe {
            let mut d = zeroed();
            let mut n = zeroed();
            assert_eq!(darwin_stat(c"/dev/null".as_ptr(), &mut d), 0);
            assert_eq!(stat(c"/dev/null".as_ptr(), &mut n), 0);
            assert_eq!(d.size, n.st_size);
            assert_eq!(d.mode, n.st_mode as u16);

            let dir = opendir(c"/dev".as_ptr());
            assert!(!dir.is_null());
            let native = loop {
                let entry = readdir(dir);
                assert!(!entry.is_null());
                if CStr::from_ptr(std::ptr::addr_of!((*entry).d_name).cast()).to_bytes() == b"null"
                {
                    break ((*entry).d_ino, (*entry).d_type);
                }
            };
            rewinddir(dir);

            let mut found = false;
            loop {
                let d = darwin_readdir(dir);
                if d.is_null() {
                    break;
                }
                if CStr::from_ptr(std::ptr::addr_of!((*d).name).cast()).to_bytes() == b"null" {
                    assert_eq!((*d).namlen, 4);
                    assert_eq!((*d).ino, native.0);
                    assert_eq!((*d).kind, native.1);
                    found = true;
                }
            }

            closedir(dir);
            assert!(found);
        }
    }

    #[test]
    fn terminal_echo_translation() {
        unsafe {
            let master = posix_openpt(O_RDWR | O_NOCTTY);
            assert!(master >= 0);
            assert_eq!(grantpt(master), 0);
            assert_eq!(unlockpt(master), 0);
            let fd = open(ptsname(master), O_RDWR | O_NOCTTY);
            assert!(fd >= 0);

            let mut d = zeroed();
            assert_eq!(darwin_tcgetattr(fd, &mut d), 0);
            d.local &= !8;
            assert_eq!(darwin_tcsetattr(fd, 0, &d), 0);

            let mut n = zeroed();
            assert_eq!(tcgetattr(fd, &mut n), 0);
            assert_eq!(n.c_lflag & ECHO, 0);

            close(fd);
            close(master);
        }
    }

    static RECEIVED: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
    extern "C" fn received(sig: i32) {
        RECEIVED.store(sig, std::sync::atomic::Ordering::SeqCst);
    }

    #[test]
    fn translated_signal_numbers() {
        unsafe {
            let a = DarwinSigaction {
                handler: received as *const () as usize,
                mask: 0,
                flags: 0,
            };
            let mut old = zeroed();
            assert_eq!(sigaction_d_fn(30, &a, &mut old), 0);

            raise(SIGUSR1);
            assert_eq!(RECEIVED.load(std::sync::atomic::Ordering::SeqCst), 30);
            assert_eq!(sigaction_d_fn(30, &old, null_mut()), 0);
        }
    }
}
