//! CoreFoundation/CoreServices subset, using the bundled macOS Unicode snapshot.

use super::{bytes, die, errno_get, errno_set, missing, units, EXECUTABLE};
use libc::*;
use std::ffi::{CStr, CString};
use std::mem::size_of;
use std::ptr::{null, null_mut};
use std::sync::Mutex;

mod tables {
    include!(concat!(env!("OUT_DIR"), "/textmap.rs"));
}

use tables::{
    TEXTMAP_CASED, TEXTMAP_CASE_IGNORABLE, TEXTMAP_FOLD, TEXTMAP_LOWER, TEXTMAP_POOL, TEXTMAP_UPPER,
};

fn mapped(table: &[[u32; 2]], cp: u32) -> Option<&'static [u16]> {
    table.binary_search_by_key(&cp, |r| r[0]).ok().map(|i| {
        let v = table[i][1];
        &TEXTMAP_POOL[(v >> 5) as usize..(v >> 5) as usize + (v & 31) as usize]
    })
}

fn in_set(table: &[[u32; 2]], cp: u32) -> bool {
    let i = table.partition_point(|r| r[1] < cp);
    i < table.len() && table[i][0] <= cp
}

fn utf16_next(s: &[u16], i: &mut usize) -> u32 {
    let mut c = s[*i] as u32;
    *i += 1;
    if (0xd800..=0xdbff).contains(&c) && *i < s.len() && (0xdc00..=0xdfff).contains(&s[*i]) {
        c = 0x10000 + ((c - 0xd800) << 10) + (s[*i] as u32 - 0xdc00);
        *i += 1;
    }
    c
}

fn utf16_put(out: &mut Vec<u16>, mut c: u32) {
    if c < 0x10000 {
        out.push(c as u16);
    } else {
        c -= 0x10000;
        out.push((0xd800 + (c >> 10)) as u16);
        out.push((0xdc00 + (c & 0x3ff)) as u16);
    }
}

pub(super) static mut CF_CONSTANT_CLASS: u8 = 0;
static mut CF_OWNED_CLASS: u8 = 0;
static mut CF_NULL_ALLOCATOR: u8 = 0;
pub(super) static mut CF_DEFAULT_ALLOCATOR: *mut c_void = null_mut();
pub(super) static mut CF_NULL_ALLOCATOR_PTR: *mut c_void =
    std::ptr::addr_of_mut!(CF_NULL_ALLOCATOR).cast();

#[repr(C)]
pub(super) struct CfString {
    isa: *mut c_void,
    flags: usize,
    data: *mut c_void,
    len: isize,
    capacity: isize,
    utf16: i32,
    external: i32,
}

unsafe fn owned(s: *const CfString) -> bool {
    (*s).isa == std::ptr::addr_of_mut!(CF_OWNED_CLASS).cast()
}

unsafe fn is_utf16(s: *const CfString) -> bool {
    if owned(s) {
        (*s).utf16 != 0
    } else {
        (*s).flags & 16 != 0
    }
}

unsafe fn new_string() -> *mut CfString {
    let s = calloc(1, size_of::<CfString>()).cast::<CfString>();
    if s.is_null() {
        die("CFString allocation");
    }
    (*s).isa = std::ptr::addr_of_mut!(CF_OWNED_CLASS).cast();
    s
}

unsafe fn cf_new_utf8(text: *const c_char) -> *mut CfString {
    let s = new_string();
    (*s).len = strlen(text) as isize;
    (*s).capacity = (*s).len;
    (*s).data = strdup(text).cast();
    s
}

pub(super) unsafe extern "C" fn cf_mainbundle() -> *mut c_void {
    std::ptr::addr_of_mut!(EXECUTABLE).cast()
}

pub(super) unsafe extern "C" fn cf_bundle_url(_bundle: *mut c_void) -> *mut CfString {
    cf_new_utf8(std::ptr::addr_of!(EXECUTABLE).cast())
}

pub(super) unsafe extern "C" fn cf_parent_url(
    _alloc: *mut c_void,
    url: *const CfString,
) -> *mut CfString {
    let s = cf_new_utf8((*url).data.cast());
    let mut last = strrchr((*s).data.cast(), b'/' as i32);
    if !last.is_null() {
        if last == (*s).data.cast() {
            last = last.add(1);
        }
        *last = 0;
    }

    (*s).len = strlen((*s).data.cast()) as isize;
    s
}

pub(super) unsafe extern "C" fn cf_path(url: *const CfString, style: isize) -> *mut CfString {
    if style != 0 {
        errno_set(EINVAL);
        null_mut()
    } else {
        cf_new_utf8((*url).data.cast())
    }
}

pub(super) unsafe extern "C" fn cf_copy(
    _alloc: *mut c_void,
    capacity: isize,
    src: *const CfString,
) -> *mut CfString {
    let s = new_string();
    (*s).len = (*src).len;
    (*s).utf16 = i32::from(is_utf16(src));
    (*s).capacity = capacity.max((*src).len);
    let unit = if (*s).utf16 != 0 { 2 } else { 1 };
    (*s).data = calloc((*s).capacity as usize + 1, unit);
    if (*s).data.is_null() {
        die("CFString copy");
    }

    memcpy((*s).data, (*src).data, (*s).len as usize * unit);
    s
}

pub(super) unsafe extern "C" fn cf_release(s: *mut CfString) {
    if !s.is_null() && owned(s) {
        if (*s).external == 0 {
            free((*s).data);
        }
        free(s.cast());
    }
}

pub(super) unsafe extern "C" fn cf_append(s: *mut CfString, other: *const CfString) {
    if is_utf16(s) || is_utf16(other) {
        missing("CFStringAppend UTF-16");
    }

    let len = (*s).len + (*other).len;
    let p = realloc((*s).data, len as usize + 1);
    if p.is_null() {
        die("CFStringAppend");
    }

    (*s).data = p;
    memcpy(
        p.cast::<u8>().add((*s).len as usize).cast(),
        (*other).data,
        (*other).len as usize,
    );
    *p.cast::<u8>().add(len as usize) = 0;
    (*s).len = len;
    (*s).capacity = len;
}

pub(super) extern "C" fn cf_system_encoding() -> u32 {
    0x08000100
}

pub(super) unsafe extern "C" fn cf_cstring(s: *const CfString, encoding: u32) -> *const c_char {
    if is_utf16(s) || (encoding != 0x08000100 && encoding != 0x600) {
        null()
    } else {
        (*s).data.cast()
    }
}

pub(super) unsafe extern "C" fn cf_external(
    _allocator: *mut c_void,
    text: *mut u16,
    n: isize,
    cap: isize,
    _deallocator: *mut c_void,
) -> *mut CfString {
    if n < 0 || cap < n {
        return null_mut();
    }
    let s = new_string();
    (*s).data = text.cast();
    (*s).len = n;
    (*s).capacity = cap;
    (*s).utf16 = 1;
    (*s).external = 1;
    s
}

fn final_sigma(s: &[u16], mut at: usize, mut next: usize) -> bool {
    while at > 0 {
        let mut j = at - 1;
        if j > 0 && (0xdc00..=0xdfff).contains(&s[j]) && (0xd800..=0xdbff).contains(&s[j - 1]) {
            j -= 1;
        }
        let mut previous = j;
        let c = utf16_next(s, &mut previous);
        at = j;
        if in_set(TEXTMAP_CASED, c) {
            break;
        }
        if !in_set(TEXTMAP_CASE_IGNORABLE, c) {
            return false;
        }
    }

    while next < s.len() {
        let c = utf16_next(s, &mut next);
        if in_set(TEXTMAP_CASED, c) {
            return false;
        }
        if !in_set(TEXTMAP_CASE_IGNORABLE, c) {
            return true;
        }
    }
    true
}

unsafe fn cf_change_case(s: *mut CfString, upper: bool) {
    let s = &mut *s;
    if s.utf16 == 0 {
        for i in 0..s.len as usize {
            let p = s.data.cast::<u8>().add(i);
            if *p < 0x80 {
                *p = if upper {
                    toupper(*p as i32)
                } else {
                    tolower(*p as i32)
                } as u8;
            }
        }
        return;
    }

    let input = units(s.data.cast(), s.len as usize);
    let mut out = Vec::with_capacity(input.len() + 16);
    let mut i = 0;

    while i < input.len() {
        let start = i;
        let c = utf16_next(input, &mut i);
        if !upper && c == 0x3a3 && start > 0 && final_sigma(input, start, i) {
            out.push(0x3c2);
        } else if let Some(rep) = mapped(if upper { TEXTMAP_UPPER } else { TEXTMAP_LOWER }, c) {
            out.extend_from_slice(rep);
        } else {
            utf16_put(&mut out, c);
        }
    }

    if out.len() > s.capacity as usize {
        if s.external == 0 {
            free(s.data);
        }
        s.data = malloc(out.len() * 2);
        if s.data.is_null() {
            die("CFString case");
        }
        s.external = 0;
        s.capacity = out.len() as isize;
    }

    memcpy(s.data, out.as_ptr().cast(), out.len() * 2);
    if out.len() < s.capacity as usize {
        *s.data.cast::<u16>().add(out.len()) = 0;
    }
    s.len = out.len() as isize;
}

pub(super) unsafe extern "C" fn cf_upper(s: *mut CfString, _locale: *mut c_void) {
    cf_change_case(s, true)
}

pub(super) unsafe extern "C" fn cf_lower(s: *mut CfString, _locale: *mut c_void) {
    cf_change_case(s, false)
}

pub(super) extern "C" fn create_encoding(base: u32, variant: u32, format: u32) -> u32 {
    base | (variant << 16) | (format << 26)
}

static ENCODINGS: Mutex<Vec<CString>> = Mutex::new(Vec::new());

fn encoding_name(e: u32) -> Option<CString> {
    if e & 0xff000000 == 0xf0000000 {
        return ENCODINGS
            .lock()
            .unwrap()
            .get((e & 0xffffff) as usize)
            .cloned();
    }

    let name = if (0x100..=0x119).contains(&(e & 0xffff)) {
        match e >> 26 {
            0 | 5 => "UTF-16LE",
            1 => "UTF-7",
            2 => "UTF-8",
            3 | 7 => "UTF-32LE",
            4 => "UTF-16BE",
            6 => "UTF-32BE",
            _ => return None,
        }
    } else {
        match e & 0xffff {
            0 => "MACINTOSH",
            1 | 0xa01 => "SHIFT_JIS",
            0x201 => "ISO-8859-1",
            0x500 => "WINDOWS-1252",
            0x600 => "ASCII",
            _ => return None,
        }
    };
    Some(CString::new(name).unwrap())
}

const ICONV_NONE: iconv_t = usize::MAX as iconv_t;
pub(super) unsafe extern "C" fn encoding_from_name(out: *mut u32, pascal: *const u8) -> i32 {
    let Ok(name) = CString::new(bytes(pascal.add(1), *pascal as usize)) else {
        return -8738;
    };
    for (n, code) in [
        ("UTF-8", 0x08000100),
        ("UTF-16LE", 0x14000100),
        ("UTF-16BE", 0x10000100),
    ] {
        if name.to_bytes().eq_ignore_ascii_case(n.as_bytes()) {
            *out = code;
            return 0;
        }
    }

    let mut cd = iconv_open(c"UCS-4LE".as_ptr(), name.as_ptr());
    if cd == ICONV_NONE {
        cd = iconv_open(c"UTF-16LE".as_ptr(), name.as_ptr());
    }
    if cd == ICONV_NONE {
        return -8738;
    }
    iconv_close(cd);

    let mut names = ENCODINGS.lock().unwrap();
    if let Some(i) = names
        .iter()
        .position(|n| n.to_bytes().eq_ignore_ascii_case(name.to_bytes()))
    {
        *out = 0xf0000000 | i as u32;
        return 0;
    }
    if names.len() == 256 {
        return -108;
    }

    *out = 0xf0000000 | names.len() as u32;
    names.push(name);
    0
}

pub(super) unsafe extern "C" fn upgrade_encoding(
    script: i16,
    _language: i16,
    _region: i16,
    _font: *const c_void,
    out: *mut u32,
) -> i32 {
    if script >= 0 {
        *out = script as u32;
        return 0;
    }

    let n = CStr::from_ptr(nl_langinfo(CODESET)).to_bytes();
    if n.len() > 255 {
        return -8738;
    }

    let mut p = [0u8; 256];
    p[0] = n.len() as u8;
    p[1..n.len() + 1].copy_from_slice(n);
    encoding_from_name(out, p.as_ptr())
}

pub(super) struct Tec {
    cd: iconv_t,
    utf8_in: bool,
    utf16_be: bool,
}

pub(super) unsafe extern "C" fn tec_create(out: *mut *mut Tec, input: u32, output: u32) -> i32 {
    *out = null_mut();
    let (Some(src), Some(dst)) = (encoding_name(input), encoding_name(output)) else {
        return -8738;
    };

    let s16 = src.to_bytes() == b"UTF-16LE" || src.to_bytes() == b"UTF-16BE";
    let d16 = dst.to_bytes() == b"UTF-16LE" || dst.to_bytes() == b"UTF-16BE";
    let mut t = Box::new(Tec {
        cd: ICONV_NONE,
        utf8_in: false,
        utf16_be: false,
    });
    if src.to_bytes() == b"UTF-8" && d16 {
        t.utf8_in = true;
        t.utf16_be = dst.to_bytes() == b"UTF-16BE";
    } else if s16 && dst.to_bytes() == b"UTF-8" {
        t.utf16_be = src.to_bytes() == b"UTF-16BE";
    } else {
        t.cd = iconv_open(dst.as_ptr(), src.as_ptr());
        if t.cd == ICONV_NONE && (s16 || d16) {
            let fallback = |s: &CStr, is16: bool| {
                if !is16 {
                    s.as_ptr()
                } else if s.to_bytes() == b"UTF-16BE" {
                    c"UCS-2BE".as_ptr()
                } else {
                    c"UCS-2LE".as_ptr()
                }
            };
            t.cd = iconv_open(fallback(&dst, d16), fallback(&src, s16));
        }
        if t.cd == ICONV_NONE {
            return -8749;
        }
    }

    *out = Box::into_raw(t);
    0
}

pub(super) unsafe extern "C" fn tec_clear(p: *mut Tec) -> i32 {
    if (*p).cd == ICONV_NONE
        || iconv((*p).cd, null_mut(), null_mut(), null_mut(), null_mut()) != usize::MAX
    {
        0
    } else {
        -8748
    }
}

pub(super) unsafe extern "C" fn tec_dispose(p: *mut Tec) -> i32 {
    let t = Box::from_raw(p);
    if t.cd == ICONV_NONE || iconv_close(t.cd) == 0 {
        0
    } else {
        -8748
    }
}

fn utf8_decode(s: &[u8]) -> (usize, u32, bool) {
    let b = s[0];
    if b < 0x80 {
        return (1, b as u32, false);
    }

    let len = match b {
        0xc2..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf4 => 4,
        _ => 1,
    };
    if len == 1 {
        return (1, 0xfffd, true);
    }
    if s.len() < len {
        return (0, 0xfffd, false);
    }

    let mut v = (b & (0x3f >> (len - 1))) as u32;
    for (i, &c) in s.iter().enumerate().take(len).skip(1) {
        if c & 0xc0 != 0x80 {
            return (i, 0xfffd, true);
        }
        v = (v << 6) | (c & 0x3f) as u32;
    }

    if (len == 3 && v < 0x800)
        || (len == 4 && !(0x10000..=0x10ffff).contains(&v))
        || (0xd800..=0xdfff).contains(&v)
    {
        (len, 0xfffd, true)
    } else {
        (len, v, false)
    }
}

fn tec_unicode(t: &Tec, input: &[u8], out: &mut [u8]) -> (i32, usize, usize) {
    let (mut i, mut o) = (0, 0);
    let mut status = 0;
    let mut fallback = false;
    if out.is_empty() || (!t.utf8_in && !input.len().is_multiple_of(2)) {
        return (-50, 0, 0);
    }

    while i < input.len() {
        let (used, c) = if t.utf8_in {
            let (u, c, f) = utf8_decode(&input[i..]);
            fallback = f;
            (u, c)
        } else {
            let unit = |at: usize| {
                let v = [input[at], input[at + 1]];
                if t.utf16_be {
                    u16::from_be_bytes(v)
                } else {
                    u16::from_le_bytes(v)
                }
            };
            let u = unit(i);
            if (0xdc00..=0xdfff).contains(&u) {
                (2, 0xfffd)
            } else if (0xd800..=0xdbff).contains(&u) {
                if input.len() - i < 4 {
                    status = -8753;
                    break;
                }
                let w = unit(i + 2);
                if !(0xdc00..=0xdfff).contains(&w) {
                    (2, 0xfffd)
                } else {
                    (
                        4,
                        0x10000 + ((u as u32 - 0xd800) << 10) + (w as u32 - 0xdc00),
                    )
                }
            } else {
                (2, u as u32)
            }
        };
        if used == 0 {
            status = -8753;
            break;
        }

        let mut buf = [0u8; 4];
        let k = if t.utf8_in {
            let mut v = Vec::with_capacity(2);
            utf16_put(&mut v, c);
            for (j, u) in v.iter().enumerate() {
                buf[j * 2..j * 2 + 2].copy_from_slice(&if t.utf16_be {
                    u.to_be_bytes()
                } else {
                    u.to_le_bytes()
                });
            }
            v.len() * 2
        } else {
            char::from_u32(c).unwrap().encode_utf8(&mut buf).len()
        };
        if o + k > out.len() {
            status = if o != 0 { -8785 } else { -8750 };
            break;
        }

        out[o..o + k].copy_from_slice(&buf[..k]);
        o += k;
        i += used;
    }

    if status == -8753 && out.len() - o < if t.utf8_in { 2 } else { 1 } {
        status = if o != 0 { -8785 } else { -8750 };
    }
    (
        if status != 0 {
            status
        } else if fallback {
            -8783
        } else {
            0
        },
        i,
        o,
    )
}

pub(super) unsafe extern "C" fn tec_convert(
    p: *mut Tec,
    input: *const c_char,
    input_len: usize,
    consumed: *mut usize,
    output: *mut c_char,
    output_len: usize,
    produced: *mut usize,
) -> i32 {
    let t = &*p;
    if t.cd == ICONV_NONE {
        let out = if output_len == 0 {
            &mut []
        } else {
            std::slice::from_raw_parts_mut(output.cast(), output_len)
        };
        let (r, i, o) = tec_unicode(t, bytes(input.cast(), input_len), out);
        *consumed = i;
        *produced = o;
        return r;
    }

    let mut src = input.cast_mut();
    let mut dst = output;
    let mut il = input_len;
    let mut ol = output_len;
    let r = iconv(t.cd, &mut src, &mut il, &mut dst, &mut ol);
    *consumed = input_len - il;
    *produced = output_len - ol;
    if r != usize::MAX {
        return 0;
    }

    match errno_get() {
        E2BIG => {
            if *produced != 0 {
                -8785
            } else {
                -8750
            }
        }
        EINVAL => -8753,
        EILSEQ => -8754,
        _ => -8748,
    }
}

static mut UC_COLLATOR: u8 = 0;
pub(super) unsafe extern "C" fn uc_create(
    locale: *mut c_void,
    variant: u32,
    options: u32,
    out: *mut *mut c_void,
) -> i32 {
    *out = null_mut();
    if !locale.is_null() || variant != 0 {
        return -50;
    }
    if options != 0x800e {
        missing("UCCreateCollator options other than 0x800E");
    }
    *out = std::ptr::addr_of_mut!(UC_COLLATOR).cast();
    0
}

pub(super) unsafe extern "C" fn uc_dispose(c: *mut *mut c_void) -> i32 {
    if !c.is_null() {
        *c = null_mut();
    }
    0
}

fn collation_key(s: &[u16]) -> Vec<u16> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        let c = utf16_next(s, &mut i);
        if (0xac00..=0xd7a3).contains(&c) {
            let v = c - 0xac00;
            out.push((0x1100 + v / 588) as u16);
            out.push((0x1161 + v % 588 / 28) as u16);
            if !v.is_multiple_of(28) {
                out.push((0x11a7 + v % 28) as u16);
            }
        } else if let Some(rep) = mapped(TEXTMAP_FOLD, c) {
            out.extend_from_slice(rep);
        } else {
            utf16_put(&mut out, c);
        }
    }
    out
}

pub(super) unsafe extern "C" fn uc_compare(
    c: *mut c_void,
    a: *const u16,
    an: usize,
    b: *const u16,
    bn: usize,
    equal: *mut u8,
    order: *mut i32,
) -> i32 {
    if c != std::ptr::addr_of_mut!(UC_COLLATOR).cast()
        || an > i32::MAX as usize
        || bn > i32::MAX as usize
    {
        return -50;
    }

    let a = units(a, an);
    let b = units(b, bn);
    let mut v = collation_key(a).cmp(&collation_key(b)) as i32;
    if !equal.is_null() {
        *equal = u8::from(v == 0);
    }
    if v == 0 {
        let (mut i, mut j) = (0, 0);
        while v == 0 && i < an && j < bn {
            let p = utf16_next(a, &mut i);
            let q = utf16_next(b, &mut j);
            if p == q {
                continue;
            }
            let up = mapped(TEXTMAP_LOWER, p).is_some();
            let uq = mapped(TEXTMAP_LOWER, q).is_some();
            v = if up != uq {
                i32::from(up) - i32::from(uq)
            } else {
                p.cmp(&q) as i32
            };
        }
        if v == 0 {
            v = an.cmp(&bn) as i32;
        }
    }

    if !order.is_null() {
        *order = v;
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unicode_conversion_statuses() {
        unsafe {
            let mut cv = null_mut();
            assert_eq!(tec_create(&mut cv, 0x08000100, 0x100), 0);
            let input = "Aé日😀".as_bytes();
            let mut out = [0u8; 128];
            let (mut used, mut produced) = (0, 0);
            assert_eq!(
                tec_convert(
                    cv,
                    input.as_ptr().cast(),
                    input.len(),
                    &mut used,
                    out.as_mut_ptr().cast(),
                    out.len(),
                    &mut produced
                ),
                0
            );
            assert_eq!(used, input.len());
            assert_eq!(
                &out[..produced],
                &[0x41, 0, 0xe9, 0, 0xe5, 0x65, 0x3d, 0xd8, 0, 0xde]
            );
            assert_eq!(tec_clear(cv), 0);
            assert_eq!(
                tec_convert(
                    cv,
                    input.as_ptr().cast(),
                    input.len(),
                    &mut used,
                    out.as_mut_ptr().cast(),
                    2,
                    &mut produced
                ),
                -8785
            );
            assert_eq!((used, produced), (1, 2));
            assert_eq!(tec_dispose(cv), 0);
        }
    }

    #[test]
    fn broken_utf8_and_partial_sequences() {
        let t = Tec {
            cd: ICONV_NONE,
            utf8_in: true,
            utf16_be: false,
        };
        let mut out = [0u8; 16];
        for (input, status, used, written) in [
            (&b"\xe0\x80\x80"[..], -8783, 3, 2),
            (&b"\xe0\x80"[..], -8753, 0, 0),
            (&b"\xffA"[..], 0, 2, 4),
            (&b"\xe0A\x80"[..], -8783, 3, 6),
        ] {
            assert_eq!(tec_unicode(&t, input, &mut out), (status, used, written));
        }
    }

    #[test]
    fn mediainfo_cf_path() {
        unsafe {
            let url = cf_new_utf8(c"/tmp/dthd/bin/encoder".as_ptr());
            let parent = cf_parent_url(null_mut(), url);
            let path = cf_path(parent, 0);
            let copy = cf_copy(null_mut(), 0, path);
            let suffix = CfString {
                isa: std::ptr::addr_of_mut!(CF_CONSTANT_CLASS).cast(),
                flags: 0x7c8,
                data: c"/libmediainfo.0.dylib".as_ptr().cast_mut().cast(),
                len: c"/libmediainfo.0.dylib".to_bytes().len() as isize,
                capacity: 0,
                utf16: 0,
                external: 0,
            };
            cf_append(copy, &suffix);
            assert_eq!(
                CStr::from_ptr(cf_cstring(copy, cf_system_encoding())),
                c"/tmp/dthd/bin/libmediainfo.0.dylib"
            );
            for s in [copy, path, parent, url] {
                cf_release(s);
            }
        }
    }

    #[test]
    #[ignore = "requires DTHD_GENERATED with the pinned Unicode snapshot"]
    fn snapshot_case_and_collation() {
        unsafe {
            assert!(!TEXTMAP_UPPER.is_empty());
            let mut chars = [0x61, 0xe4, 0x3c3, 0x7a];
            let s = cf_external(null_mut(), chars.as_mut_ptr(), 4, 4, null_mut());
            cf_upper(s, null_mut());
            assert_eq!(chars, [0x41, 0xc4, 0x3a3, 0x5a]);
            cf_lower(s, null_mut());
            assert_eq!(chars, [0x61, 0xe4, 0x3c3, 0x7a]);
            cf_release(s);
            let mut coll = null_mut();
            assert_eq!(uc_create(null_mut(), 0, 0x800e, &mut coll), 0);
            for (a, b) in [
                (&[0xe9][..], &[0x65, 0x301][..]),
                (&[0x41][..], &[0x61][..]),
                (&[0x41][..], &[0xff21][..]),
            ] {
                let (mut equal, mut order) = (0, 0);
                assert_eq!(
                    uc_compare(
                        coll,
                        a.as_ptr(),
                        a.len(),
                        b.as_ptr(),
                        b.len(),
                        &mut equal,
                        &mut order
                    ),
                    0
                );
                assert_eq!(equal, 1);
            }
            assert_eq!(uc_dispose(&mut coll), 0);
        }
    }
}
