use super::{loader, os, sync, text};
use crate::{fft, math};
use libc::c_void;

pub(super) fn resolve(name: &[u8]) -> Option<*mut c_void> {
    Some(match name {
        b"fopen" => os::darwin_fopen as *const () as *mut c_void,
        b"fdopen" => os::darwin_fdopen as *const () as *mut c_void,
        b"fclose" => os::darwin_fclose as *const () as *mut c_void,
        b"localeconv" => os::darwin_localeconv as *const () as *mut c_void,
        b"pthread_key_create" => sync::darwin_pthread_key_create as *const () as *mut c_void,
        b"setlocale" => os::darwin_setlocale as *const () as *mut c_void,
        b"pthread_getschedparam" => sync::darwin_pthread_getsched as *const () as *mut c_void,
        b"pthread_setschedparam" => sync::darwin_pthread_setsched as *const () as *mut c_void,
        b"dlsym" => os::darwin_dlsym as *const () as *mut c_void,
        b"dladdr" => os::darwin_dladdr as *const () as *mut c_void,
        b"CFBundleGetMainBundle" => text::cf_mainbundle as *const () as *mut c_void,
        b"CFBundleCopyExecutableURL" => text::cf_bundle_url as *const () as *mut c_void,
        b"CFURLCreateCopyDeletingLastPathComponent" => {
            text::cf_parent_url as *const () as *mut c_void
        }
        b"CFURLCopyFileSystemPath" => text::cf_path as *const () as *mut c_void,
        b"CFStringCreateMutableCopy" => text::cf_copy as *const () as *mut c_void,
        b"CFStringAppend" => text::cf_append as *const () as *mut c_void,
        b"CFStringGetSystemEncoding" => text::cf_system_encoding as *const () as *mut c_void,
        b"CFStringGetCStringPtr" => text::cf_cstring as *const () as *mut c_void,
        b"CFRelease" => text::cf_release as *const () as *mut c_void,
        b"CFStringCreateMutableWithExternalCharactersNoCopy" => {
            text::cf_external as *const () as *mut c_void
        }
        b"CFStringLowercase" => text::cf_lower as *const () as *mut c_void,
        b"CFStringUppercase" => text::cf_upper as *const () as *mut c_void,
        b"__CFConstantStringClassReference" => {
            std::ptr::addr_of_mut!(text::CF_CONSTANT_CLASS).cast()
        }
        b"kCFAllocatorDefault" => std::ptr::addr_of_mut!(text::CF_DEFAULT_ALLOCATOR).cast(),
        b"kCFAllocatorNull" => std::ptr::addr_of_mut!(text::CF_NULL_ALLOCATOR_PTR).cast(),
        b"CreateTextEncoding" => text::create_encoding as *const () as *mut c_void,
        b"TECGetTextEncodingFromInternetName" => {
            text::encoding_from_name as *const () as *mut c_void
        }
        b"UpgradeScriptInfoToTextEncoding" => text::upgrade_encoding as *const () as *mut c_void,
        b"TECCreateConverter" => text::tec_create as *const () as *mut c_void,
        b"TECDisposeConverter" => text::tec_dispose as *const () as *mut c_void,
        b"TECClearConverterContextInfo" => text::tec_clear as *const () as *mut c_void,
        b"TECConvertText" => text::tec_convert as *const () as *mut c_void,
        b"UCCreateCollator" => text::uc_create as *const () as *mut c_void,
        b"UCDisposeCollator" => text::uc_dispose as *const () as *mut c_void,
        b"UCCompareText" => text::uc_compare as *const () as *mut c_void,
        b"mach_task_self_" => std::ptr::addr_of_mut!(super::MACH_TASK).cast(),
        b"semaphore_create" => sync::mach_sem_create as *const () as *mut c_void,
        b"semaphore_destroy" => sync::mach_sem_destroy as *const () as *mut c_void,
        b"semaphore_wait" => sync::mach_sem_wait as *const () as *mut c_void,
        b"semaphore_signal" => sync::mach_sem_signal as *const () as *mut c_void,
        b"pthread_mach_thread_np" => sync::darwin_pthread_mach as *const () as *mut c_void,
        b"thread_policy_set" => sync::darwin_thread_policy as *const () as *mut c_void,
        b"pthread_attr_setdetachstate" => {
            sync::pthread_attr_set_detach_state as *const () as *mut c_void
        }
        b"mmap" => os::darwin_mmap as *const () as *mut c_void,
        b"shm_open" => os::darwin_shm_open as *const () as *mut c_void,
        b"pathconf" => os::darwin_pathconf as *const () as *mut c_void,
        b"signal" => os::darwin_signal as *const () as *mut c_void,
        b"sigaction" => os::sigaction_d_fn as *const () as *mut c_void,
        b"tcgetattr" => os::darwin_tcgetattr as *const () as *mut c_void,
        b"tcsetattr" => os::darwin_tcsetattr as *const () as *mut c_void,
        b"readdir$INODE64" => os::darwin_readdir as *const () as *mut c_void,
        b"readdir_r$INODE64" => os::darwin_readdir_r as *const () as *mut c_void,
        b"fcntl" => os::darwin_fcntl as *const () as *mut c_void,
        b"vDSP_create_fftsetup" => fft::fft_create as *const () as *mut c_void,
        b"vDSP_create_fftsetupD" => fft::fft_create as *const () as *mut c_void,
        b"vDSP_destroy_fftsetup" => fft::fft_destroy as *const () as *mut c_void,
        b"vDSP_destroy_fftsetupD" => fft::fft_destroy as *const () as *mut c_void,
        b"vDSP_fft_zrip" => fft::fft_real_f as *const () as *mut c_void,
        b"vDSP_fft_zripD" => fft::fft_real_d as *const () as *mut c_void,
        b"vDSP_ctoz" => fft::ctoz_f as *const () as *mut c_void,
        b"vDSP_ctozD" => fft::ctoz_d as *const () as *mut c_void,
        b"vDSP_ztoc" => fft::ztoc_f as *const () as *mut c_void,
        b"vDSP_ztocD" => fft::ztoc_d as *const () as *mut c_void,
        b"vDSP_vdpsp" => fft::vdpsp as *const () as *mut c_void,
        b"vDSP_vspdp" => fft::vspdp as *const () as *mut c_void,
        b"_ZNKSt3__16locale9use_facetERNS0_2idE" => {
            os::darwin_use_facet as *const () as *mut c_void
        }
        b"atexit" => libc::atexit as *const () as *mut c_void,
        b"___chkstk_darwin" => loader::darwin_chkstk as *const () as *mut c_void,
        b"__stack_chk_guard" => std::ptr::addr_of_mut!(super::STACK_GUARD).cast(),
        b"__error" => os::errno_location as *const () as *mut c_void,
        b"_NSGetExecutablePath" => os::executable_path as *const () as *mut c_void,
        b"CCRandomGenerateBytes" => os::random_bytes as *const () as *mut c_void,
        b"mach_absolute_time" => sync::nanotime as *const () as *mut c_void,
        b"mach_timebase_info" => os::timebase as *const () as *mut c_void,
        b"AudioConvertNanosToHostTime" => os::audio_hosttime as *const () as *mut c_void,
        b"sysctl" => os::sysctl_full as *const () as *mut c_void,
        b"sysconf" => os::sysconf_full as *const () as *mut c_void,
        b"memset_pattern16" => os::memset_pattern16 as *const () as *mut c_void,
        b"__bzero" => super::bzero as *const () as *mut c_void,
        b"powf" => math::darwin_powf as *const () as *mut c_void,
        b"__exp10" => super::exp10 as *const () as *mut c_void,
        b"__exp10f" => super::exp10f as *const () as *mut c_void,
        b"__sincos_stret" => os::darwin_sincos as *const () as *mut c_void,
        b"__sincosf_stret" => math::sincos_f as *const () as *mut c_void,
        b"__tolower" => libc::tolower as *const () as *mut c_void,
        b"__toupper" => libc::toupper as *const () as *mut c_void,
        b"__maskrune" => os::maskrune as *const () as *mut c_void,
        b"_DefaultRuneLocale" => std::ptr::addr_of_mut!(super::RUNELOCALE).cast(),
        b"pthread_mutex_init" => sync::mutex_init as *const () as *mut c_void,
        b"pthread_mutex_lock" => sync::mutex_lock as *const () as *mut c_void,
        b"pthread_mutex_unlock" => sync::mutex_unlock as *const () as *mut c_void,
        b"pthread_mutex_destroy" => sync::mutex_destroy as *const () as *mut c_void,
        b"pthread_cond_init" => sync::condvar_init as *const () as *mut c_void,
        b"pthread_cond_wait" => sync::condvar_wait as *const () as *mut c_void,
        b"pthread_cond_signal" => sync::condvar_signal as *const () as *mut c_void,
        b"pthread_cond_broadcast" => sync::condvar_broadcast as *const () as *mut c_void,
        b"pthread_cond_destroy" => sync::condvar_destroy as *const () as *mut c_void,
        b"pthread_rwlock_init" => sync::rwlock_init as *const () as *mut c_void,
        b"pthread_rwlock_rdlock" => sync::rwlock_read as *const () as *mut c_void,
        b"pthread_rwlock_wrlock" => sync::rwlock_write as *const () as *mut c_void,
        b"pthread_rwlock_unlock" => sync::rwlock_unlock as *const () as *mut c_void,
        b"pthread_rwlock_destroy" => sync::rwlock_destroy as *const () as *mut c_void,
        b"pthread_once" => sync::pthread_once_darwin as *const () as *mut c_void,
        b"_ZNSt3__15mutex4lockEv" => sync::mutex_lock as *const () as *mut c_void,
        b"_ZNSt3__15mutex6unlockEv" => sync::mutex_unlock as *const () as *mut c_void,
        b"_ZNSt3__15mutexD1Ev" => sync::mutex_destroy as *const () as *mut c_void,
        b"_ZNSt3__115recursive_mutex4lockEv" => sync::mutex_lock as *const () as *mut c_void,
        b"_ZNSt3__115recursive_mutex6unlockEv" => sync::mutex_unlock as *const () as *mut c_void,
        b"_ZNSt3__115recursive_mutexC1Ev" => sync::recursive_mutex_init as *const () as *mut c_void,
        b"_ZNSt3__115recursive_mutexD1Ev" => sync::mutex_destroy as *const () as *mut c_void,
        b"_ZNSt3__118condition_variable4waitERNS_11unique_lockINS_5mutexEEE" => {
            sync::condition_variable_wait as *const () as *mut c_void
        }
        b"_ZNSt3__118condition_variable10notify_allEv" => {
            sync::condvar_broadcast as *const () as *mut c_void
        }
        b"_ZNSt3__118condition_variable10notify_oneEv" => {
            sync::condvar_signal as *const () as *mut c_void
        }
        b"_ZNSt3__118condition_variableD1Ev" => sync::condvar_destroy as *const () as *mut c_void,
        b"_ZNSt3__118condition_variable15__do_timed_waitERNS_11unique_lockINS_5mutexEEENS_6chrono10time_pointINS5_12system_clockENS5_8durationIxNS_5ratioILl1ELl1000000000EEEEEEE" => {
            sync::condition_variable_timed_wait as *const () as *mut c_void
        }
        b"dispatch_semaphore_create" => sync::dispatch_create as *const () as *mut c_void,
        b"dispatch_semaphore_wait" => sync::dispatch_wait as *const () as *mut c_void,
        b"dispatch_semaphore_signal" => sync::dispatch_signal as *const () as *mut c_void,
        b"dispatch_release" => sync::darwin_dispatch_release as *const () as *mut c_void,
        b"dispatch_time" => sync::darwin_dispatch_time as *const () as *mut c_void,
        b"stat$INODE64" => os::darwin_stat as *const () as *mut c_void,
        b"lstat$INODE64" => os::darwin_lstat as *const () as *mut c_void,
        b"fstat$INODE64" => os::darwin_fstat as *const () as *mut c_void,
        b"open" => os::darwin_open as *const () as *mut c_void,
        b"fgetpos" => os::darwin_fgetpos as *const () as *mut c_void,
        b"fsetpos" => os::darwin_fsetpos as *const () as *mut c_void,
        b"dlopen" => os::darwin_dlopen as *const () as *mut c_void,
        b"_ZNSt3__16chrono12system_clock3nowEv" => os::system_now as *const () as *mut c_void,
        b"_ZNSt3__16chrono12system_clock9to_time_tERKNS0_10time_pointIS1_NS0_8durationIxNS_5ratioILl1ELl1000000EEEEEEE" => {
            os::system_to_time as *const () as *mut c_void
        }
        _ => return None,
    })
}
