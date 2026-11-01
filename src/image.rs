//! Bounded parser for the classic x86-64 Mach-O metadata understood by the runtime.

use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub const BASE: u64 = 0x100000000;

pub const MAX_FILE: usize = 256 * 1024 * 1024;
const MAX_RECORDS: usize = 1_000_000;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Segment {
    pub name: String,
    pub addr: u64,
    pub size: u64,
    pub offset: u64,
    pub filesize: u64,
    pub prot: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Section {
    pub name: String,
    pub addr: u64,
    pub size: u64,
    pub offset: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Binding {
    pub address: u64,
    pub symbol: String,
    pub addend: i64,
    pub kind: u8,
}

#[derive(Clone, Debug)]
pub struct Compact {
    pub start: u32,
    pub length: u32,
    pub encoding: u32,
    pub lsda: u32,
}

#[derive(Default, Debug)]
pub struct Image {
    pub segments: Vec<Segment>,
    pub sections: Vec<Section>,
    pub dylibs: Vec<String>,
    pub bindings: Vec<Binding>,
    pub initializers: Vec<u64>,
    pub entry: u64,
    pub compact: Vec<Compact>,
    pub personalities: Vec<u32>,
    pub blockers: Vec<String>,
    pub review: Vec<String>,
}

pub fn region(b: &[u8], offset: u64, size: u64) -> Result<&[u8]> {
    let end = offset.checked_add(size).context("range overflow")?;
    ensure!(end <= b.len() as u64, "range outside input");
    Ok(&b[offset as usize..end as usize])
}

fn u16le(b: &[u8], offset: u64) -> Result<u16> {
    Ok(u16::from_le_bytes(region(b, offset, 2)?.try_into()?))
}

fn u32le(b: &[u8], offset: u64) -> Result<u32> {
    Ok(u32::from_le_bytes(region(b, offset, 4)?.try_into()?))
}

fn u64le(b: &[u8], offset: u64) -> Result<u64> {
    Ok(u64::from_le_bytes(region(b, offset, 8)?.try_into()?))
}

fn name(b: &[u8]) -> Result<String> {
    let b = b.split(|v| *v == 0).next().unwrap_or_default();
    ensure!(b.is_ascii(), "non-ASCII metadata name");
    Ok(std::str::from_utf8(b)?.into())
}

impl Image {
    fn reject(&mut self, message: impl Into<String>) {
        let message = message.into();
        if !self.blockers.contains(&message) {
            self.blockers.push(message);
        }
    }

    fn contains(&self, address: u64, size: u64, protection: u32) -> bool {
        self.segments.iter().any(|s| {
            s.name != "__PAGEZERO"
                && address >= s.addr
                && address
                    .checked_add(size)
                    .is_some_and(|end| end <= s.addr + s.size)
                && s.prot & protection == protection
        })
    }

    pub fn parse(&mut self, b: &[u8]) -> Result<()> {
        ensure!(b.len() <= MAX_FILE, "input exceeds 256 MiB screening limit");
        region(b, 0, 32)?;
        ensure!(
            (u32le(b, 0)?, u32le(b, 4)?, u32le(b, 12)?) == (0xfeedfacf, 0x1000007, 2),
            "need thin x86-64 MH_EXECUTE Mach-O"
        );

        let count = u32le(b, 16)?;
        let size = u32le(b, 20)?;
        ensure!(
            count <= 4096.min(size / 8),
            "invalid or excessive load-command count"
        );
        let commands = region(b, 32, size as u64)?;
        let mut offset = 0;
        let mut dyld = None;
        let mut entry = None;
        if u32le(b, 24)? & 0x20000 != 0 {
            self.reject("MH_ALLOW_STACK_EXECUTION is unsupported");
        }

        for _ in 0..count {
            let command = u32le(commands, offset)?;
            let length = u32le(commands, offset + 4)? as u64;
            ensure!(
                length >= 8 && length.is_multiple_of(8),
                "invalid load-command size"
            );
            let chunk = region(commands, offset, length)?;
            offset += length;
            match command {
                0x19 => self.segment(b, chunk)?,
                0x80000022 => {
                    ensure!(
                        dyld.is_none() && length == 48,
                        "invalid or duplicate dyld info"
                    );
                    let mut v = [0u32; 10];
                    for (i, x) in v.iter_mut().enumerate() {
                        *x = u32le(chunk, 8 + i as u64 * 4)?;
                    }
                    for p in v.chunks_exact(2) {
                        region(b, p[0] as u64, p[1] as u64)?;
                    }
                    dyld = Some(v);
                }
                0x80000028 => {
                    ensure!(
                        entry.is_none() && length == 24,
                        "invalid or duplicate LC_MAIN"
                    );
                    entry = Some(u64le(chunk, 8)?);
                    if u64le(chunk, 16)? != 0 {
                        self.review
                            .push("LC_MAIN requests a custom stack size".into());
                    }
                }
                0xc | 0x80000018 | 0x8000001f => {
                    let start = u32le(chunk, 8)? as u64;
                    ensure!(24 <= start && start < length, "invalid dylib name offset");
                    let string = region(chunk, start, length - start)?;
                    ensure!(string.contains(&0), "unterminated dylib name");
                    self.dylibs.push(name(string)?);
                    if command != 0xc {
                        self.review
                            .push("weak or re-exported dylib requires review".into());
                    }
                }
                0x80000034 | 0x80000033 => {
                    self.reject("chained fixups or standalone exports trie are unsupported")
                }
                0x21 | 0x2c => self.reject("encryption load commands are unsupported"),
                0x2 | 0xb | 0xe | 0x1b | 0x1d | 0x24 | 0x26 | 0x29 | 0x2a | 0x32 | 0x8000001c => {}
                _ => self
                    .review
                    .push(format!("unrecognized load command {command:#x}")),
            }
        }
        ensure!(
            offset == size as u64,
            "load-command sizes do not match header"
        );

        let dyld = dyld.context("need classic dyld info and LC_MAIN")?;
        let entry = entry.context("need classic dyld info and LC_MAIN")?;
        let text: Vec<_> = self
            .segments
            .iter()
            .filter(|s| s.name == "__TEXT")
            .collect();
        ensure!(text.len() == 1, "need exactly one __TEXT segment");
        let text = text[0].clone();
        if text.addr != BASE || text.offset != 0 {
            self.reject("__TEXT must use base 0x100000000 and file offset zero");
        }

        self.entry = BASE.checked_add(entry).context("entry overflow")?;
        ensure!(
            self.contains(self.entry, 1, 4),
            "entry point is not executable"
        );
        ensure!(
            entry < text.filesize,
            "entry point is outside file-backed __TEXT"
        );

        let mut ranges: Vec<_> = self
            .segments
            .iter()
            .filter(|s| s.size > 0)
            .map(|s| (s.addr, s.addr + s.size))
            .collect();
        ranges.sort_unstable();
        ensure!(
            ranges.windows(2).all(|r| r[0].1 <= r[1].0),
            "overlapping segments"
        );

        self.parse_bindings(region(b, dyld[2] as u64, dyld[3] as u64)?)?;
        self.parse_bindings(region(b, dyld[6] as u64, dyld[7] as u64)?)?;
        ensure!(!self.bindings.is_empty(), "no imports found");

        for s in &self.sections {
            if s.name == "__mod_init_func" {
                ensure!(s.size % 8 == 0, "invalid initializer array size");
                let data = region(b, s.offset, s.size)?;
                for c in data.chunks_exact(8) {
                    let a = u64::from_le_bytes(c.try_into()?);
                    ensure!(self.contains(a, 1, 4), "initializer is not executable");
                    self.initializers.push(a);
                }
            }
        }

        self.unwind(b)
    }

    fn segment(&mut self, b: &[u8], chunk: &[u8]) -> Result<()> {
        region(chunk, 0, 72)?;
        let label = name(region(chunk, 8, 16)?)?;
        ensure!(
            label.starts_with("__")
                && label.len() > 2
                && label
                    .bytes()
                    .skip(2)
                    .all(|c| c.is_ascii_alphanumeric() || c == b'_'),
            "unsupported segment name"
        );
        ensure!(self.segments.len() < 16, "too many segments");
        ensure!(
            !self
                .segments
                .iter()
                .any(|s| s.name.eq_ignore_ascii_case(&label)),
            "segment names collide after ELF section conversion"
        );

        let addr = u64le(chunk, 24)?;
        let size = u64le(chunk, 32)?;
        let offset = u64le(chunk, 40)?;
        let filesize = u64le(chunk, 48)?;
        let prot = u32le(chunk, 60)?;
        let count = u32le(chunk, 64)?;
        let flags = u32le(chunk, 68)?;
        ensure!(
            chunk.len() as u64 == 72 + count as u64 * 80,
            "invalid segment section count"
        );
        ensure!(filesize <= size, "invalid segment size");
        let end = addr.checked_add(size).context("invalid segment size")?;
        region(b, offset, filesize)?;
        if label == "__PAGEZERO" {
            ensure!(
                addr == 0 && size == BASE && filesize == 0 && prot == 0,
                "invalid page zero"
            );
        } else {
            ensure!(
                addr >= BASE && end <= BASE + (1 << 32),
                "segment outside 32-bit image offsets"
            );
            if addr % 4096 != 0 || size % 4096 != 0 || ![1, 3, 5].contains(&prot) || size == 0 {
                self.reject(format!(
                    "{label}: unsupported segment alignment, size or protection"
                ));
            }
            if prot & 2 == 0 && size > filesize.div_ceil(4096) * 4096 {
                self.reject(format!(
                    "{label}: read-only zero-fill extends beyond the last mapped file page"
                ));
            }
            if flags & !0x10 != 0 {
                self.review.push(format!("{label}: nonzero segment flags"));
            }
        }

        self.segments.push(Segment {
            name: label.clone(),
            addr,
            size,
            offset,
            filesize,
            prot,
        });

        for i in 0..count {
            let p = 72 + i as u64 * 80;
            let section = name(region(chunk, p, 16)?)?;
            let owner = name(region(chunk, p + 16, 16)?)?;
            let start = u64le(chunk, p + 32)?;
            let length = u64le(chunk, p + 40)?;
            let fileoff = u32le(chunk, p + 48)? as u64;
            let reloff = u32le(chunk, p + 56)?;
            let reloc = u32le(chunk, p + 60)?;
            let kind = u32le(chunk, p + 64)? & 255;
            ensure!(
                owner == label
                    && start >= addr
                    && start.checked_add(length).is_some_and(|e| e <= end),
                "section outside segment"
            );

            let zero = [1, 0xc, 0x12].contains(&kind);
            if section == "__mod_init_func" || section == "__unwind_info" {
                ensure!(!zero, "runtime metadata must be file-backed");
            }
            if !zero {
                ensure!(
                    fileoff >= offset
                        && fileoff
                            .checked_add(length)
                            .is_some_and(|e| e <= offset + filesize),
                    "section outside file-backed segment"
                );
                ensure!(
                    start - addr == fileoff - offset,
                    "section file/virtual offsets disagree"
                );
            }

            if (0x11..=0x15).contains(&kind) {
                self.reject("Mach-O thread-local storage is unsupported");
            }
            if kind == 0x16 {
                self.reject("offset-based initializers are unsupported");
            }
            if kind == 0xa {
                self.review
                    .push("Mach-O termination functions are not registered".into());
            }
            if reloc != 0 {
                region(b, reloff as u64, reloc as u64 * 8)?;
                self.reject("section relocations are unsupported");
            }
            if kind == 9 && section != "__mod_init_func" {
                self.reject("initializer section name is not recognized by the generator");
            }

            self.sections.push(Section {
                name: section,
                addr: start,
                size: length,
                offset: fileoff,
            });
        }

        Ok(())
    }

    fn parse_bindings(&mut self, b: &[u8]) -> Result<()> {
        let mut p = 0;
        let mut seg = 0;
        let mut off = 0u64;
        let mut symbol = String::new();
        let mut addend = 0;
        let mut kind = 1;

        while p < b.len() {
            let value = b[p];
            p += 1;
            let op = value & 0xf0;
            let imm = value & 15;
            match op {
                0 => {
                    addend = 0;
                    kind = 1;
                }
                0x10 | 0x30 => {}
                0x20 => {
                    leb(b, &mut p, false)?;
                }
                0x40 => {
                    let end = b[p..]
                        .iter()
                        .position(|&c| c == 0)
                        .context("unterminated import name")?
                        + p;
                    symbol = name(&b[p..end])?;
                    p = end + 1;
                }
                0x50 => kind = imm,
                0x60 => addend = leb(b, &mut p, true)? as i64,
                0x70 => {
                    seg = imm as usize;
                    off = leb(b, &mut p, false)?;
                }
                0x80 => off = off.wrapping_add(leb(b, &mut p, false)?),
                0x90 | 0xa0 | 0xb0 | 0xc0 => {
                    let (count, skip) = if op == 0xc0 {
                        (leb(b, &mut p, false)?, leb(b, &mut p, false)?)
                    } else {
                        (1, 0)
                    };
                    ensure!(
                        count <= MAX_RECORDS.saturating_sub(self.bindings.len()) as u64,
                        "too many bindings"
                    );

                    for _ in 0..count {
                        ensure!(
                            !symbol.is_empty()
                                && symbol
                                    .bytes()
                                    .all(|c| c.is_ascii_alphanumeric() || b"_.$".contains(&c)),
                            "invalid import spelling"
                        );

                        let s = self.segments.get(seg).context("invalid binding segment")?;
                        let address = s.addr.wrapping_add(off);
                        ensure!(
                            s.name != "__PAGEZERO"
                                && s.size >= 8
                                && address >= s.addr
                                && address <= s.addr + s.size - 8,
                            "binding outside segment"
                        );
                        if kind != 1 {
                            self.reject(format!(
                                "unsupported binding type {kind}; need pointer bindings"
                            ));
                        }

                        self.bindings.push(Binding {
                            address,
                            symbol: symbol.clone(),
                            addend,
                            kind,
                        });
                        off = off.wrapping_add(8).wrapping_add(skip);
                    }

                    if op == 0xa0 {
                        off = off.wrapping_add(leb(b, &mut p, false)?);
                    } else if op == 0xb0 {
                        off = off.wrapping_add(imm as u64 * 8);
                    }
                }
                _ => bail!("unsupported binding opcode {value:#x}"),
            }
        }

        Ok(())
    }

    fn unwind(&mut self, b: &[u8]) -> Result<()> {
        let sections: Vec<_> = self
            .sections
            .iter()
            .filter(|s| s.name == "__unwind_info")
            .collect();
        ensure!(
            sections.len() == 1,
            "need exactly one compact unwind section"
        );

        let section = sections[0];
        let data = region(b, section.offset, section.size)?;
        let version = u32le(data, 0)?;
        let co = u32le(data, 4)?;
        let cn = u32le(data, 8)?;
        let po = u32le(data, 12)?;
        let pn = u32le(data, 16)?;
        let io = u32le(data, 20)?;
        let count = u32le(data, 24)?;
        ensure!(
            version == 1 && cn <= 256 && (2..=MAX_RECORDS as u32).contains(&count) && pn <= 3,
            "unsupported compact unwind header"
        );

        let common: Vec<u32> = (0..cn)
            .map(|i| u32le(data, co as u64 + i as u64 * 4))
            .collect::<Result<_>>()?;
        for i in 0..pn {
            let p = u32le(data, po as u64 + i as u64 * 4)?;
            ensure!(
                self.contains(BASE + p as u64, 8, 0),
                "personality pointer outside image"
            );
            self.personalities.push(p);
        }

        let indices: Vec<[u32; 3]> = (0..count)
            .map(|i| {
                let p = io as u64 + i as u64 * 12;
                Ok([u32le(data, p)?, u32le(data, p + 4)?, u32le(data, p + 8)?])
            })
            .collect::<Result<_>>()?;

        let mut entries = Vec::new();
        let mut lsdas = BTreeMap::new();
        let mut lsda_count = 0;
        for pair in indices.windows(2) {
            let [start, page, lo] = pair[0];
            let [end, _, hi] = pair[1];
            ensure!(
                start < end && lo <= hi && (hi - lo) % 8 == 0,
                "invalid unwind index"
            );
            lsda_count += (hi - lo) as usize / 8;
            ensure!(lsda_count <= MAX_RECORDS, "too many LSDA records");
            for p in (lo..hi).step_by(8) {
                let f = u32le(data, p as u64)?;
                let lsda = u32le(data, p as u64 + 4)?;
                ensure!(
                    self.contains(BASE + lsda as u64, 1, 0),
                    "LSDA outside image"
                );
                lsdas.insert(f, lsda);
            }

            let page = page as u64;
            let kind = u32le(data, page)?;
            let eo = u16le(data, page + 4)? as u64;
            let ec = u16le(data, page + 6)? as usize;
            ensure!(entries.len() + ec <= MAX_RECORDS, "too many unwind records");
            let old = entries.len();
            match kind {
                2 => {
                    for j in 0..ec {
                        let p = page + eo + j as u64 * 8;
                        entries.push((u32le(data, p)?, u32le(data, p + 4)?));
                    }
                }
                3 => {
                    let encoff = u16le(data, page + 8)? as u64;
                    let enccount = u16le(data, page + 10)? as usize;
                    let mut enc = common.clone();
                    for j in 0..enccount {
                        enc.push(u32le(data, page + encoff + j as u64 * 4)?);
                    }
                    for j in 0..ec {
                        let raw = u32le(data, page + eo + j as u64 * 4)?;
                        let encoding = *enc
                            .get((raw >> 24) as usize)
                            .context("invalid unwind encoding index")?;
                        entries.push((
                            start
                                .checked_add(raw & 0xffffff)
                                .context("unwind offset overflow")?,
                            encoding,
                        ));
                    }
                }
                _ => bail!("unsupported compact unwind page kind {kind}"),
            }
            ensure!(
                entries[old..].iter().all(|(f, _)| start <= *f && *f < end),
                "unwind function outside index range"
            );
        }

        entries.push((indices.last().unwrap()[0], 0));
        for pair in entries.windows(2) {
            let (start, encoding) = pair[0];
            let (end, _) = pair[1];
            ensure!(
                start < end && self.contains(BASE + start as u64, (end - start) as u64, 4),
                "invalid unwind function range"
            );
            if encoding == 0 {
                continue;
            }
            if (encoding >> 24) & 15 != 1 {
                self.reject("compact unwind uses modes other than RBP (1)");
            }
            ensure!(
                (encoding >> 28) & 3 <= pn,
                "invalid unwind personality index"
            );
            let lsda = *lsdas.get(&start).unwrap_or(&0);
            ensure!(encoding & 0x40000000 == 0 || lsda != 0, "missing LSDA");
            ensure!(
                (0..5).all(|i| (encoding >> (3 * i)) & 7 != 7),
                "invalid RBP saved register"
            );
            self.compact.push(Compact {
                start,
                length: end - start,
                encoding,
                lsda,
            });
        }
        ensure!(!self.compact.is_empty(), "no usable unwind records");

        Ok(())
    }

    pub fn assess(b: &[u8], capabilities: &Value, pinned_hash: &str) -> Value {
        let mut image = Self::default();
        if let Err(e) = image.parse(b) {
            image.reject(e.to_string());
        }

        let imports: BTreeSet<_> = image.bindings.iter().map(|b| b.symbol.clone()).collect();
        let known: BTreeSet<_> = capabilities["imports"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        let dylibs: BTreeSet<_> = capabilities["dylibs"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();

        let unknown: Vec<_> = imports
            .iter()
            .filter(|s| !known.contains(s.as_str()))
            .collect();
        let unknown_dylibs: BTreeSet<_> = image
            .dylibs
            .iter()
            .filter(|s| !dylibs.contains(s.as_str()))
            .collect();
        if !unknown.is_empty() {
            image
                .review
                .push("imports outside the validated inventory".into());
        }
        if !unknown_dylibs.is_empty() {
            image
                .review
                .push("dylibs outside the validated inventory".into());
        }

        image.review.sort();
        image.review.dedup();
        let digest = crate::digest(b);
        let status = if !image.blockers.is_empty() {
            "reject"
        } else if !image.review.is_empty() {
            "review"
        } else {
            "candidate"
        };

        json!({
            "status": status,
            "sha256": digest,
            "matches_payload_pin": digest == pinned_hash,
            "blockers": image.blockers,
            "review": image.review,
            "unknown_imports": unknown,
            "unknown_dylibs": unknown_dylibs,
            "counts": {
                "imports": imports.len(),
                "bindings": image.bindings.len(),
                "unwind_records": image.compact.len(),
            },
        })
    }
}

fn leb(b: &[u8], pos: &mut usize, signed: bool) -> Result<u64> {
    let mut value = 0i128;
    for shift in (0..70).step_by(7) {
        let byte = *b.get(*pos).context("truncated LEB128")?;
        *pos += 1;
        value |= ((byte & 127) as i128) << shift;
        if byte & 128 == 0 {
            if signed && byte & 64 != 0 {
                value -= 1i128 << (shift + 7);
            }
            ensure!(
                if signed {
                    (i64::MIN as i128..=i64::MAX as i128).contains(&value)
                } else {
                    (0..=u64::MAX as i128).contains(&value)
                },
                "LEB128 overflow"
            );
            return Ok(value as u64);
        }
    }
    bail!("oversized LEB128")
}
