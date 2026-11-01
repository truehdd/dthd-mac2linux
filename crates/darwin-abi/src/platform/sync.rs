use super::{die, errno_get};
use libc::*;
use std::cell::UnsafeCell;
use std::collections::HashMap;
use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};
use std::sync::{Mutex, OnceLock};

#[derive(Clone, Copy)]
enum SyncKind {
    Mutex,
    RecursiveMutex,
    Condvar,
    RwLock,
    Once,
}

union NativeSync {
    mutex: pthread_mutex_t,
    cond: pthread_cond_t,
    rw: pthread_rwlock_t,
    once: pthread_once_t,
}

struct SyncObject(UnsafeCell<NativeSync>);

// Access to a native object is serialized by its pthread API; the registry only
// protects creation/removal. Destroying an object still in use is a caller error.
unsafe impl Send for SyncObject {}
static SYNCS: OnceLock<Mutex<HashMap<usize, Box<SyncObject>>>> = OnceLock::new();

unsafe fn sync_object(key: *mut c_void, kind: SyncKind) -> *mut NativeSync {
    let mut map = SYNCS.get_or_init(Default::default).lock().unwrap();
    let value = map.entry(key as usize).or_insert_with(|| {
        let value = Box::new(SyncObject(UnsafeCell::new(zeroed())));
        let p = value.0.get();

        match kind {
            SyncKind::Mutex | SyncKind::RecursiveMutex => {
                let mut attr = zeroed();
                pthread_mutexattr_init(&mut attr);
                if matches!(kind, SyncKind::RecursiveMutex) {
                    pthread_mutexattr_settype(&mut attr, PTHREAD_MUTEX_RECURSIVE);
                }
                pthread_mutex_init(&mut (*p).mutex, &attr);
                pthread_mutexattr_destroy(&mut attr);
            }
            SyncKind::Condvar => {
                pthread_cond_init(&mut (*p).cond, null());
            }
            SyncKind::RwLock => {
                pthread_rwlock_init(&mut (*p).rw, null());
            }
            SyncKind::Once => {}
        }

        value
    });
    value.0.get()
}

unsafe fn destroy_sync_object(key: *mut c_void, kind: SyncKind) -> i32 {
    let mut map = SYNCS.get_or_init(Default::default).lock().unwrap();
    let Some(value) = map.get(&(key as usize)) else {
        return 0;
    };

    let p = value.0.get();
    let result = match kind {
        SyncKind::Mutex | SyncKind::RecursiveMutex => pthread_mutex_destroy(&mut (*p).mutex),
        SyncKind::Condvar => pthread_cond_destroy(&mut (*p).cond),
        SyncKind::RwLock => pthread_rwlock_destroy(&mut (*p).rw),
        SyncKind::Once => 0,
    };

    if result == 0 {
        map.remove(&(key as usize));
    }
    result
}

pub(super) unsafe extern "C" fn mutex_init(p: *mut c_void, _a: *mut c_void) -> i32 {
    sync_object(p, SyncKind::Mutex);
    0
}

pub(super) unsafe extern "C" fn mutex_lock(p: *mut c_void) -> i32 {
    pthread_mutex_lock(&mut (*sync_object(p, SyncKind::Mutex)).mutex)
}

pub(super) unsafe extern "C" fn mutex_unlock(p: *mut c_void) -> i32 {
    pthread_mutex_unlock(&mut (*sync_object(p, SyncKind::Mutex)).mutex)
}

pub(super) unsafe extern "C" fn mutex_destroy(p: *mut c_void) -> i32 {
    destroy_sync_object(p, SyncKind::Mutex)
}

pub(super) unsafe extern "C" fn recursive_mutex_init(p: *mut c_void) {
    sync_object(p, SyncKind::RecursiveMutex);
}

pub(super) unsafe extern "C" fn condvar_init(p: *mut c_void, _a: *mut c_void) -> i32 {
    sync_object(p, SyncKind::Condvar);
    0
}

pub(super) unsafe extern "C" fn condvar_wait(p: *mut c_void, m: *mut c_void) -> i32 {
    pthread_cond_wait(
        &mut (*sync_object(p, SyncKind::Condvar)).cond,
        &mut (*sync_object(m, SyncKind::Mutex)).mutex,
    )
}

pub(super) unsafe extern "C" fn condvar_signal(p: *mut c_void) -> i32 {
    pthread_cond_signal(&mut (*sync_object(p, SyncKind::Condvar)).cond)
}

pub(super) unsafe extern "C" fn condvar_broadcast(p: *mut c_void) -> i32 {
    pthread_cond_broadcast(&mut (*sync_object(p, SyncKind::Condvar)).cond)
}

pub(super) unsafe extern "C" fn condvar_destroy(p: *mut c_void) -> i32 {
    destroy_sync_object(p, SyncKind::Condvar)
}

pub(super) unsafe extern "C" fn rwlock_init(p: *mut c_void, _a: *mut c_void) -> i32 {
    sync_object(p, SyncKind::RwLock);
    0
}

pub(super) unsafe extern "C" fn rwlock_read(p: *mut c_void) -> i32 {
    pthread_rwlock_rdlock(&mut (*sync_object(p, SyncKind::RwLock)).rw)
}

pub(super) unsafe extern "C" fn rwlock_write(p: *mut c_void) -> i32 {
    pthread_rwlock_wrlock(&mut (*sync_object(p, SyncKind::RwLock)).rw)
}

pub(super) unsafe extern "C" fn rwlock_unlock(p: *mut c_void) -> i32 {
    pthread_rwlock_unlock(&mut (*sync_object(p, SyncKind::RwLock)).rw)
}

pub(super) unsafe extern "C" fn rwlock_destroy(p: *mut c_void) -> i32 {
    destroy_sync_object(p, SyncKind::RwLock)
}

pub(super) unsafe extern "C" fn pthread_once_darwin(p: *mut c_void, f: extern "C" fn()) -> i32 {
    pthread_once(&mut (*sync_object(p, SyncKind::Once)).once, f)
}

pub(super) unsafe extern "C" fn condition_variable_wait(p: *mut c_void, lock: *const *mut c_void) {
    condvar_wait(p, *lock);
}

pub(super) unsafe extern "C" fn condition_variable_timed_wait(
    p: *mut c_void,
    lock: *const *mut c_void,
    ns: i64,
) {
    let t = timespec {
        tv_sec: ns / 1_000_000_000,
        tv_nsec: ns % 1_000_000_000,
    };
    pthread_cond_timedwait(
        &mut (*sync_object(p, SyncKind::Condvar)).cond,
        &mut (*sync_object(*lock, SyncKind::Mutex)).mutex,
        &t,
    );
}

pub(super) unsafe extern "C" fn nanotime() -> u64 {
    let mut t = zeroed();
    clock_gettime(CLOCK_MONOTONIC, &mut t);
    t.tv_sec as u64 * 1_000_000_000 + t.tv_nsec as u64
}

pub(super) unsafe extern "C" fn dispatch_create(n: isize) -> *mut sem_t {
    let p = calloc(1, size_of::<sem_t>()).cast::<sem_t>();
    if p.is_null() {
        die("semaphore allocation");
    }
    sem_init(p, 0, n as u32);
    p
}

pub(super) unsafe extern "C" fn dispatch_wait(p: *mut sem_t, t: u64) -> isize {
    if t == u64::MAX {
        return sem_wait(p) as isize;
    }
    if t == 0 {
        return sem_trywait(p) as isize;
    }

    let mut rt = zeroed();
    clock_gettime(CLOCK_REALTIME, &mut rt);
    let ns = t.wrapping_sub(nanotime()) as i64 + rt.tv_nsec;
    rt.tv_sec += ns / 1_000_000_000;
    rt.tv_nsec = ns % 1_000_000_000;
    sem_timedwait(p, &rt) as isize
}

pub(super) unsafe extern "C" fn dispatch_signal(p: *mut sem_t) -> isize {
    sem_post(p);
    0
}

pub(super) unsafe extern "C" fn darwin_dispatch_release(p: *mut sem_t) {
    sem_destroy(p);
    free(p.cast());
}

pub(super) unsafe extern "C" fn darwin_dispatch_time(t: u64, delta: i64) -> u64 {
    if t == u64::MAX {
        t
    } else {
        (if t == 0 { nanotime() } else { t }).wrapping_add(delta as u64)
    }
}

struct MachSem(UnsafeCell<sem_t>);

unsafe impl Send for MachSem {}
type MachRegistry = (u32, HashMap<u32, Box<MachSem>>);
static MACH_SEMS: OnceLock<Mutex<MachRegistry>> = OnceLock::new();

fn mach_registry() -> &'static Mutex<MachRegistry> {
    MACH_SEMS.get_or_init(|| Mutex::new((1, HashMap::new())))
}

pub(super) unsafe extern "C" fn mach_sem_create(
    _task: u32,
    out: *mut u32,
    policy: i32,
    value: i32,
) -> i32 {
    if out.is_null() || value < 0 || policy != 0 {
        return 4;
    }

    let sem = Box::new(MachSem(UnsafeCell::new(zeroed())));
    if sem_init(sem.0.get(), 0, value as u32) != 0 {
        return 5;
    }

    let mut guard = mach_registry().lock().unwrap();
    let id = guard.0;
    guard.0 = guard.0.wrapping_add(1);
    guard.1.insert(id, sem);
    *out = id;
    0
}

unsafe fn mach_sem_get(id: u32) -> *mut sem_t {
    mach_registry()
        .lock()
        .unwrap()
        .1
        .get(&id)
        .map_or(null_mut(), |s| s.0.get())
}

pub(super) unsafe extern "C" fn mach_sem_wait(id: u32) -> i32 {
    let p = mach_sem_get(id);
    if p.is_null() {
        4
    } else if sem_wait(p) == 0 {
        0
    } else if errno_get() == EINTR {
        14
    } else {
        5
    }
}

pub(super) unsafe extern "C" fn mach_sem_signal(id: u32) -> i32 {
    let p = mach_sem_get(id);
    if p.is_null() {
        4
    } else if sem_post(p) == 0 {
        0
    } else {
        5
    }
}

pub(super) unsafe extern "C" fn mach_sem_destroy(_task: u32, id: u32) -> i32 {
    let mut g = mach_registry().lock().unwrap();
    let Some(p) = g.1.get(&id) else {
        return 4;
    };
    if sem_destroy(p.0.get()) != 0 {
        return 5;
    }

    g.1.remove(&id);
    0
}

pub(super) unsafe extern "C" fn darwin_pthread_key_create(
    out: *mut u64,
    destructor: Option<unsafe extern "C" fn(*mut c_void)>,
) -> i32 {
    let mut key = 0;
    let r = pthread_key_create(&mut key, destructor);
    if r == 0 {
        *out = key as u64;
    }
    r
}

pub(super) unsafe extern "C" fn darwin_pthread_mach(t: pthread_t) -> u32 {
    if pthread_equal(t, pthread_self()) != 0 {
        syscall(SYS_gettid) as u32
    } else {
        0
    }
}

#[repr(C)]
struct SchedAttr {
    size: u32,
    policy: u32,
    flags: u64,
    nice: i32,
    priority: u32,
    runtime: u64,
    deadline: u64,
    period: u64,
}

pub(super) unsafe extern "C" fn darwin_thread_policy(
    tid: u32,
    flavor: u32,
    policy: *const u32,
    count: u32,
) -> i32 {
    if tid == 0 || flavor != 2 || count != 4 || policy.is_null() {
        return 4;
    }

    let a = SchedAttr {
        size: size_of::<SchedAttr>() as u32,
        policy: 6,
        flags: 0,
        nice: 0,
        priority: 0,
        runtime: *policy.add(1) as u64,
        deadline: *policy.add(2) as u64,
        period: *policy as u64,
    };
    if a.runtime == 0 || a.runtime > a.deadline || a.deadline > a.period {
        return 4;
    }

    if syscall(SYS_sched_setattr, tid, &a, 0) == 0 {
        0
    } else {
        5
    }
}

pub(super) unsafe extern "C" fn pthread_attr_set_detach_state(
    p: *mut pthread_attr_t,
    state: i32,
) -> i32 {
    if state != 1 && state != 2 {
        EINVAL
    } else {
        pthread_attr_setdetachstate(
            p,
            if state == 2 {
                PTHREAD_CREATE_DETACHED
            } else {
                PTHREAD_CREATE_JOINABLE
            },
        )
    }
}

pub(super) unsafe extern "C" fn darwin_pthread_getsched(
    t: pthread_t,
    p: *mut i32,
    s: *mut sched_param,
) -> i32 {
    let r = pthread_getschedparam(t, p, s);
    if r == 0 {
        if *p == SCHED_OTHER {
            *p = 1;
        } else if *p == SCHED_FIFO {
            *p = 4;
        }
    }
    r
}

pub(super) unsafe extern "C" fn darwin_pthread_setsched(
    t: pthread_t,
    p: i32,
    s: *const sched_param,
) -> i32 {
    let p = match p {
        1 => SCHED_OTHER,
        4 => SCHED_FIFO,
        2 => 2,
        _ => return EINVAL,
    };
    pthread_setschedparam(t, p, s)
}

#[cfg(test)]
mod tests {
    use super::*;

    unsafe extern "C" {
        fn pthread_attr_getdetachstate(attr: *const pthread_attr_t, state: *mut i32) -> i32;
    }

    #[test]
    fn synchronization_handles_survive_address_reuse() {
        unsafe {
            let mut storage = [0u64; 32];
            let key = storage.as_mut_ptr().cast();

            for _ in 0..20 {
                assert_eq!(mutex_init(key, null_mut()), 0);
                assert_eq!(mutex_lock(key), 0);
                assert_eq!(mutex_unlock(key), 0);
                assert_eq!(mutex_destroy(key), 0);

                assert_eq!(rwlock_init(key, null_mut()), 0);
                assert_eq!(rwlock_read(key), 0);
                assert_eq!(rwlock_unlock(key), 0);
                assert_eq!(rwlock_destroy(key), 0);
            }

            recursive_mutex_init(key);
            assert_eq!(mutex_lock(key), 0);
            assert_eq!(mutex_lock(key), 0);
            assert_eq!(mutex_unlock(key), 0);
            assert_eq!(mutex_unlock(key), 0);
            assert_eq!(mutex_destroy(key), 0);
        }
    }

    #[test]
    fn tls_key_fills_all_eight_darwin_bytes() {
        unsafe {
            let mut key = u64::MAX;
            assert_eq!(darwin_pthread_key_create(&mut key, None), 0);
            assert!(key <= u32::MAX as u64);

            let mut v = 7;
            assert_eq!(
                pthread_setspecific(key as u32, (&mut v as *mut i32).cast()),
                0
            );
            assert_eq!(pthread_getspecific(key as u32), (&mut v as *mut i32).cast());
            assert_eq!(pthread_key_delete(key as u32), 0);
        }
    }

    #[test]
    fn semaphore_lifetime_and_cross_thread_signal() {
        unsafe {
            let mut id = 0;
            assert_eq!(mach_sem_create(0, &mut id, 0, 0), 0);

            let thread = std::thread::spawn(move || assert_eq!(mach_sem_signal(id), 0));
            assert_eq!(mach_sem_wait(id), 0);
            thread.join().unwrap();
            assert_eq!(mach_sem_destroy(0, id), 0);
            assert_eq!(mach_sem_signal(id), 4);

            let sem = dispatch_create(1);
            assert_eq!(dispatch_wait(sem, 0), 0);
            assert_ne!(dispatch_wait(sem, 0), 0);
            assert_eq!(dispatch_signal(sem), 0);
            assert_eq!(dispatch_wait(sem, u64::MAX), 0);
            darwin_dispatch_release(sem);
        }
    }

    #[test]
    fn pthread_enum_translations() {
        unsafe {
            let mut attr = zeroed();
            assert_eq!(pthread_attr_init(&mut attr), 0);
            for (d, n) in [(1, PTHREAD_CREATE_JOINABLE), (2, PTHREAD_CREATE_DETACHED)] {
                assert_eq!(pthread_attr_set_detach_state(&mut attr, d), 0);
                let mut state = 0;
                assert_eq!(pthread_attr_getdetachstate(&attr, &mut state), 0);
                assert_eq!(state, n);
            }
            pthread_attr_destroy(&mut attr);

            let mut policy = 0;
            let mut param = zeroed();
            assert_eq!(
                darwin_pthread_getsched(pthread_self(), &mut policy, &mut param),
                0
            );
            assert_eq!(policy, 1);
            assert_eq!(darwin_pthread_setsched(pthread_self(), 1, &param), 0);
        }
    }
}
