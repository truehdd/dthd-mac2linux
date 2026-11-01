//! Synchronous C++ exception frames translated from Darwin compact unwind.

use super::{
    image::{COMPACT, PERSONALITIES, SEGMENTS},
    trace,
};
use libc::*;
use std::mem::zeroed;

unsafe extern "C" {
    fn __register_frame(p: *const u8);
}

fn uleb(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let b = (v & 127) as u8;
        v >>= 7;
        out.push(b | if v != 0 { 128 } else { 0 });
        if v == 0 {
            break;
        }
    }
}

fn finish(out: &mut Vec<u8>, start: usize) {
    while !out.len().is_multiple_of(8) {
        out.push(0);
    }

    let n = (out.len() - start - 4) as u32;
    out[start..start + 4].copy_from_slice(&n.to_le_bytes());
}

pub(super) unsafe fn register() {
    let mut out = Vec::new();
    let mut fdes = Vec::new();

    for personality in 0..=PERSONALITIES.len() {
        let cie = out.len();
        out.extend_from_slice(&[0; 8]);
        out.push(1);
        out.extend_from_slice(if personality != 0 { b"zPLR\0" } else { b"zR\0" });
        uleb(&mut out, 1);
        out.extend_from_slice(&[0x78, 16]);
        if personality != 0 {
            uleb(&mut out, 11);
            out.push(0);
            let address = 0x100000000 + PERSONALITIES[personality - 1] as usize;
            out.extend_from_slice(&std::ptr::read_unaligned(address as *const u64).to_le_bytes());
            out.extend_from_slice(&[0, 0]);
        } else {
            uleb(&mut out, 1);
            out.push(0);
        }
        out.extend_from_slice(&[0x0c, 7, 8, 0x90, 1]);
        finish(&mut out, cie);

        for e in COMPACT {
            if ((e.encoding >> 28) & 3) as usize != personality {
                continue;
            }
            assert_eq!(
                (e.encoding >> 24) & 15,
                1,
                "unsupported compact unwind mode"
            );

            let fde = out.len();
            fdes.push(fde);
            out.extend_from_slice(&0u32.to_le_bytes());
            out.extend_from_slice(&((out.len() - cie) as u32).to_le_bytes());
            out.extend_from_slice(&(0x100000000u64 + e.start as u64).to_le_bytes());
            out.extend_from_slice(&(e.length as u64).to_le_bytes());
            if personality != 0 {
                uleb(&mut out, 8);
                out.extend_from_slice(
                    &(if e.lsda == 0 {
                        0
                    } else {
                        0x100000000 + e.lsda as u64
                    })
                    .to_le_bytes(),
                );
            } else {
                uleb(&mut out, 0);
            }

            out.extend_from_slice(&[0x0c, 6, 16, 0x86, 2]);
            let off = (e.encoding >> 16) & 255;
            let regs = [0, 3, 12, 13, 14, 15, 6];
            for j in 0..5 {
                let r = (e.encoding >> (3 * j)) & 7;
                if r != 0 {
                    out.push(0x80 | regs[r as usize]);
                    uleb(&mut out, off.wrapping_add(2).wrapping_sub(j) as u64);
                }
            }
            finish(&mut out, fde);
        }
    }

    out.extend_from_slice(&[0; 4]);
    let out = Box::leak(out.into_boxed_slice());
    for fde in fdes {
        __register_frame(out.as_ptr().add(fde));
    }

    if trace() {
        eprintln!(
            "registered {} unwind records ({} bytes)",
            COMPACT.len(),
            out.len()
        );
    }
}

struct Filter {
    callback: unsafe extern "C" fn(*mut dl_phdr_info, usize, *mut c_void) -> i32,
    data: *mut c_void,
}

unsafe extern "C" {
    fn __real_dl_iterate_phdr(
        callback: unsafe extern "C" fn(*mut dl_phdr_info, usize, *mut c_void) -> i32,
        data: *mut c_void,
    ) -> i32;
}

unsafe extern "C" fn filtered(info: *mut dl_phdr_info, size: usize, arg: *mut c_void) -> i32 {
    let f = &*(arg.cast::<Filter>());
    let base = SEGMENTS.first().map_or(usize::MAX, |s| s.addr);
    let mut ph: [Elf64_Phdr; 64] = zeroed();
    if (*info).dlpi_phnum as usize > ph.len() {
        return (f.callback)(info, size, f.data);
    }

    let mut n = 0;
    for i in 0..(*info).dlpi_phnum as usize {
        let p = &*(*info).dlpi_phdr.add(i);
        if p.p_type == PT_LOAD && (*info).dlpi_addr + p.p_vaddr >= base as u64 {
            continue;
        }
        ph[n] = *p;
        n += 1;
    }

    let mut copy = std::ptr::read(info);
    copy.dlpi_phdr = ph.as_ptr();
    copy.dlpi_phnum = n as u16;
    (f.callback)(&mut copy, size, f.data)
}

#[no_mangle]
pub(super) unsafe extern "C" fn __wrap_dl_iterate_phdr(
    callback: unsafe extern "C" fn(*mut dl_phdr_info, usize, *mut c_void) -> i32,
    data: *mut c_void,
) -> i32 {
    let mut f = Filter { callback, data };
    __real_dl_iterate_phdr(filtered, (&mut f as *mut Filter).cast())
}
