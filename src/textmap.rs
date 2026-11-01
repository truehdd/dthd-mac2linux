//! Generate the pinned Unicode snapshot directly through macOS framework APIs.

use anyhow::Result;
use std::path::Path;

#[cfg(not(target_os = "macos"))]
pub fn run(_repo: &Path, _output: &Path) -> Result<()> {
    anyhow::bail!("textmap generation requires macOS")
}

#[cfg(target_os = "macos")]
pub fn run(repo: &Path, output: &Path) -> Result<()> {
    native::run(repo, output)
}

#[cfg(target_os = "macos")]
mod native {
    use super::*;
    use anyhow::{ensure, Context};
    use std::{
        ffi::c_void,
        fmt::Write,
        ptr::{null, null_mut},
    };

    #[repr(C)]
    struct Range {
        location: isize,
        length: isize,
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFStringCreateMutable(alloc: *const c_void, capacity: isize) -> *mut c_void;
        fn CFStringAppendCharacters(s: *mut c_void, chars: *const u16, count: isize);
        fn CFStringUppercase(s: *mut c_void, locale: *const c_void);
        fn CFStringLowercase(s: *mut c_void, locale: *const c_void);
        fn CFStringNormalize(s: *mut c_void, form: isize);
        fn CFStringFold(s: *mut c_void, flags: usize, locale: *const c_void);
        fn CFStringGetLength(s: *const c_void) -> isize;
        fn CFStringGetCharacters(s: *const c_void, range: Range, out: *mut u16);
        fn CFRelease(s: *const c_void);
    }

    #[link(name = "CoreServices", kind = "framework")]
    unsafe extern "C" {
        fn UCCreateCollator(
            locale: *const c_void,
            variant: u32,
            options: u32,
            out: *mut *mut c_void,
        ) -> i32;
        fn UCDisposeCollator(coll: *mut *mut c_void) -> i32;
        fn UCCompareText(
            coll: *mut c_void,
            a: *const u16,
            an: usize,
            b: *const u16,
            bn: usize,
            equal: *mut u8,
            order: *mut i32,
        ) -> i32;
    }

    struct Cf(*mut c_void);
    impl Drop for Cf {
        fn drop(&mut self) {
            unsafe { CFRelease(self.0) }
        }
    }

    impl Cf {
        fn new(text: &[u16]) -> Result<Self> {
            unsafe {
                let s = CFStringCreateMutable(null(), 0);
                ensure!(!s.is_null(), "CFString allocation failed");
                CFStringAppendCharacters(s, text.as_ptr(), text.len() as isize);
                Ok(Self(s))
            }
        }

        fn get(&self) -> Result<Vec<u16>> {
            unsafe {
                let n = CFStringGetLength(self.0);
                ensure!((0..=31).contains(&n), "mapping exceeds 31 UTF-16 units");

                let mut out = vec![0; n as usize];
                CFStringGetCharacters(
                    self.0,
                    Range {
                        location: 0,
                        length: n,
                    },
                    out.as_mut_ptr(),
                );
                Ok(out)
            }
        }

        fn lower(&self) {
            unsafe { CFStringLowercase(self.0, null()) }
        }
    }

    struct Collator(*mut c_void);
    impl Drop for Collator {
        fn drop(&mut self) {
            unsafe {
                UCDisposeCollator(&mut self.0);
            }
        }
    }

    impl Collator {
        fn new() -> Result<Self> {
            let mut p = null_mut();
            ensure!(
                unsafe { UCCreateCollator(null(), 0, 0x800e, &mut p) } == 0,
                "UCCreateCollator failed"
            );
            Ok(Self(p))
        }

        fn equivalent(&self, a: &[u16], b: &[u16]) -> Result<bool> {
            let mut eq = 0;
            let mut order = 0;
            ensure!(
                unsafe {
                    UCCompareText(
                        self.0,
                        a.as_ptr(),
                        a.len(),
                        b.as_ptr(),
                        b.len(),
                        &mut eq,
                        &mut order,
                    )
                } == 0,
                "UCCompareText failed"
            );
            Ok(eq != 0)
        }
    }

    fn encode(cp: u32) -> Vec<u16> {
        let mut buf = [0; 2];
        char::from_u32(cp).unwrap().encode_utf16(&mut buf).to_vec()
    }

    fn lower_at(text: &[u16], at: usize) -> Result<u16> {
        let s = Cf::new(text)?;
        s.lower();
        Ok(*s.get()?.get(at).unwrap_or(&0))
    }

    fn mark(set: &mut Vec<[u32; 2]>, cp: u32) {
        if let Some(last) = set.last_mut() {
            if last[1] + 1 == cp {
                last[1] = cp;
                return;
            }
        }

        set.push([cp, cp]);
    }

    fn in_set(set: &[[u32; 2]], cp: u32) -> bool {
        let i = set.partition_point(|r| r[1] < cp);
        i < set.len() && set[i][0] <= cp
    }

    fn key(map: &[(u32, Vec<u16>)], cp: u32) -> Vec<u16> {
        if (0xac00..=0xd7a3).contains(&cp) {
            let s = cp - 0xac00;
            let mut out = vec![(0x1100 + s / 588) as u16, (0x1161 + s % 588 / 28) as u16];
            if !s.is_multiple_of(28) {
                out.push((0x11a7 + s % 28) as u16);
            }
            out
        } else if let Ok(i) = map.binary_search_by_key(&cp, |r| r.0) {
            map[i].1.clone()
        } else {
            encode(cp)
        }
    }

    pub fn run(repo: &Path, output: &Path) -> Result<()> {
        let coll = Collator::new()?;
        let mut maps: [Vec<(u32, Vec<u16>)>; 3] = Default::default();
        let mut ranges: [Vec<[u32; 2]>; 2] = Default::default();
        let mut rejected = 0;
        let mut ignored = 0;

        for cp in 0..=0x10ffff {
            if (0xd800..=0xdfff).contains(&cp) {
                continue;
            }

            let u = encode(cp);
            for (m, map) in maps.iter_mut().enumerate().take(2) {
                let s = Cf::new(&u)?;
                unsafe {
                    if m == 0 {
                        CFStringUppercase(s.0, null())
                    } else {
                        CFStringLowercase(s.0, null())
                    }
                }
                let r = s.get()?;
                if r != u {
                    map.push((cp, r));
                }
            }

            let mut t = vec![b'A' as u16, 0x3a3];
            t.extend_from_slice(&u);
            if lower_at(&t, 1)? == 0x3c3 {
                mark(&mut ranges[0], cp);
            } else {
                t.push(b'a' as u16);
                if lower_at(&t, 1)? == 0x3c3 {
                    mark(&mut ranges[1], cp);
                }
            }

            if (0xac00..=0xd7a3).contains(&cp) {
                continue;
            }

            let mut pair = u.clone();
            pair.push(b'a' as u16);
            if coll.equivalent(&pair, &[b'a' as u16])? {
                maps[2].push((cp, Vec::new()));
                ignored += 1;
                continue;
            }

            let s = Cf::new(&u)?;
            unsafe {
                CFStringNormalize(s.0, 1);
                CFStringFold(s.0, 1 | 256, null());
            }
            let r = s.get()?;
            if r == u {
                continue;
            }
            if coll.equivalent(&u, &r)? {
                maps[2].push((cp, r));
                continue;
            }

            let s = Cf::new(&u)?;
            unsafe {
                CFStringNormalize(s.0, 1);
            }
            s.lower();
            let r = s.get()?;
            if r != u && coll.equivalent(&u, &r)? {
                maps[2].push((cp, r));
            } else {
                rejected += 1;
            }
        }

        let mut sigma_wrong = 0;
        for cp in 0..=0x10ffff {
            if (0xd800..=0xdfff).contains(&cp) {
                continue;
            }
            let mut t = encode(cp);
            let s = Cf::new(&t)?;
            s.lower();
            let before = s.get()?.len();
            t.push(0x3a3);
            if (lower_at(&t, before)? == 0x3c2)
                != (in_set(&ranges[0], cp) || in_set(&ranges[1], cp))
            {
                sigma_wrong += 1;
            }
        }

        let mut wrong = 0;
        for x in 0..0x250 {
            for y in 0..0x250 {
                let ours = key(&maps[2], x) == key(&maps[2], y);
                let mut sx = encode(x);
                sx.push(b'b' as u16);
                let mut sy = encode(y);
                sy.push(b'b' as u16);
                if ours != coll.equivalent(&sx, &sy)? {
                    wrong += 1;
                }
            }
        }

        eprintln!(
            "upper {}, lower {}, fold {} ({ignored} ignorable), \
             {rejected} rejected candidates; {} comparison pairs, {wrong} wrong; \
             final sigma {sigma_wrong} wrong",
            maps[0].len(),
            maps[1].len(),
            maps[2].len(),
            0x250 * 0x250,
        );
        ensure!(wrong == 0 && sigma_wrong == 0, "Unicode self-check failed");

        let version = std::process::Command::new("sw_vers")
            .arg("-productVersion")
            .output()?;
        ensure!(version.status.success(), "sw_vers failed");
        let version = std::str::from_utf8(&version.stdout)?.trim();
        // The pinned snapshot includes this legacy header.
        let mut text = format!(
            "/* Generated by tools/gen-textmap.c on macOS {version}; do not edit. Code points whose\n\x20\
             * CFStringUppercase, CFStringLowercase or UCCompareText (options 0x800E) key differs from\n\x20\
             * themselves; values are offset << 5 | length into textmap_pool. */\n"
        );
        text.push_str("static const uint16_t textmap_pool[]={");
        let mut pool = 0;
        for map in &maps {
            for (_, row) in map {
                for unit in row {
                    if pool != 0 {
                        text.push(',');
                    }
                    write!(text, "0x{unit:x}")?;
                    pool += 1;
                }
            }
        }
        if pool == 0 {
            text.push('0');
        }
        text.push_str("};\n");

        pool = 0;
        for (name, map) in ["textmap_upper", "textmap_lower", "textmap_fold"]
            .iter()
            .zip(&maps)
        {
            write!(text, "static const uint32_t {name}[][2]={{")?;
            for (i, (cp, row)) in map.iter().enumerate() {
                if i != 0 {
                    text.push(',');
                }
                write!(text, "{{0x{cp:x},0x{:x}}}", (pool << 5) | row.len())?;
                pool += row.len();
            }
            text.push_str("};\n");
        }

        for (name, set) in ["textmap_cased", "textmap_case_ignorable"]
            .iter()
            .zip(&ranges)
        {
            write!(text, "static const uint32_t {name}[][2]={{")?;
            for (i, [start, end]) in set.iter().enumerate() {
                if i != 0 {
                    text.push(',');
                }
                write!(text, "{{0x{start:x},0x{end:x}}}")?;
            }
            text.push_str("};\n");
        }

        let lock = crate::build::lock(repo)?;
        let expected = lock["generated_inputs"]["textmap.c.inc"]
            .as_str()
            .context("textmap pin")?;
        let digest = crate::digest(text.as_bytes());
        ensure!(
            digest == expected,
            "Unicode snapshot differs: {digest} (expected {expected})"
        );

        let parent = output
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        std::fs::create_dir_all(parent)?;
        let mut temp = tempfile::NamedTempFile::new_in(parent)?;
        std::io::Write::write_all(&mut temp, text.as_bytes())?;
        temp.persist(output)?;
        println!("Unicode snapshot SHA-256 matches: {digest}");
        Ok(())
    }
}
