//! Generate image metadata and assembly.

use crate::image::{Image, MAX_FILE};
use anyhow::{ensure, Context, Result};
use regex::Regex;
use std::{
    fs,
    io::{Read, Write},
    path::Path,
};

pub fn read_payload(path: &Path) -> Result<Vec<u8>> {
    let mut data = Vec::new();
    fs::File::open(path)
        .with_context(|| format!("open {}", path.display()))?
        .take(MAX_FILE as u64 + 1)
        .read_to_end(&mut data)?;
    ensure!(
        data.len() <= MAX_FILE,
        "input exceeds 256 MiB screening limit"
    );
    Ok(data)
}

pub fn prepare(payload: &Path, out: &Path) -> Result<Image> {
    let data = read_payload(payload)?;
    let mut image = Image::default();
    image.parse(&data)?;
    ensure!(
        image.blockers.is_empty(),
        "unsupported payload: {}",
        image.blockers.join("; ")
    );

    fs::create_dir_all(out)?;
    let mut code = String::from("// Generated Mach-O metadata.\n");
    code.push_str("pub(crate) static SEGMENTS: &[Segment] = &[\n");
    for s in &image.segments {
        if s.name != "__PAGEZERO" {
            code.push_str(&format!(
                "Segment {{ addr: {}, size: {}, prot: {} }},\n",
                s.addr, s.size, s.prot
            ));
        }
    }

    code.push_str("];\npub(crate) static BINDINGS: &[Binding] = &[\n");
    for b in &image.bindings {
        code.push_str(&format!(
            "Binding {{ addr: {}, name: c{:?}, addend: {}, kind: {} }},\n",
            b.address, b.symbol, b.addend, b.kind
        ));
    }

    code.push_str(&format!(
        r#"];
pub(crate) static INITIALIZERS: &[usize] = &{:?};
pub(crate) const ENTRY: usize = {};
pub(crate) static PERSONALITIES: &[u32] = &{:?};
pub(crate) static COMPACT: &[Compact] = &[
"#,
        image.initializers, image.entry, image.personalities,
    ));
    for e in &image.compact {
        code.push_str(&format!(
            "Compact {{ start: {}, length: {}, encoding: {}, lsda: {} }},\n",
            e.start, e.length, e.encoding, e.lsda
        ));
    }
    code.push_str("];\n");

    fs::write(out.join("image.rs"), code)?;
    fs::write(out.join("dee_dthd_encoder.macho"), &data)?;

    let manifest = serde_json::json!({
        "sha256": crate::digest(&data),
        "source": payload,
        "segments": image.segments,
        "sections": image.sections,
        "dylibs": image.dylibs,
        "entry": image.entry,
        "initializers": image.initializers,
        "bindings": image.bindings,
    });
    fs::write(
        out.join("manifest.json"),
        format!("{}\n", serde_json::to_string_pretty(&manifest)?),
    )?;

    Ok(image)
}

pub fn imports(image: &Image, hooks: &str, out: &Path) -> Result<Vec<String>> {
    let pattern = Regex::new(r#"b"([A-Za-z0-9_.$]+)"\s*=>"#)?;
    let mut hooked: std::collections::BTreeSet<_> = pattern
        .captures_iter(hooks)
        .map(|c| c[1].to_owned())
        .collect();
    hooked.extend(["__stdoutp", "__stderrp", "__stdinp"].map(str::to_owned));

    let mut names = std::collections::BTreeMap::new();
    for b in &image.bindings {
        if b.symbol == "dyld_stub_binder" {
            continue;
        }

        let name = b.symbol.strip_prefix('_').unwrap_or(&b.symbol);
        if !hooked.contains(name) {
            names.insert(name.to_owned(), name.split('$').next().unwrap().to_owned());
        }
    }

    let mut asm = String::from("/* Generated static imports. */\n.section .rodata\n");
    for (i, (name, _)) in names.iter().enumerate() {
        asm.push_str(&format!(".Limport_name{i}: .asciz {name:?}\n"));
    }
    asm.push_str(
        r#".balign 8
.global static_import_count
.hidden static_import_count
.type static_import_count,@object
static_import_count:
"#,
    );
    asm.push_str(&format!(
        ".quad {}\n.size static_import_count,8\n",
        names.len()
    ));
    asm.push_str(
        r#".section .data.rel.ro.local,"aw",@progbits
.balign 8
.global static_imports
.hidden static_imports
.type static_imports,@object
static_imports:
"#,
    );
    for (i, (_, target)) in names.iter().enumerate() {
        if target == "hypot" {
            asm.push_str(".symver dthd_hypot,hypot@GLIBC_2.2.5\n");
            asm.push_str(&format!(".quad .Limport_name{i},dthd_hypot\n"));
        } else {
            asm.push_str(&format!(".quad .Limport_name{i},{target}\n"));
        }
    }
    asm.push_str(
        ".size static_imports,.-static_imports\n.section .note.GNU-stack,\"\",@progbits\n",
    );
    fs::write(out.join("imports.S"), asm)?;

    let mut asm = String::from("/* Generated Mach-O segment mappings. */\n");
    let mut flags = Vec::new();
    for s in &image.segments {
        if s.name == "__PAGEZERO" {
            continue;
        }

        let section = format!(".dthd{}", s.name.to_lowercase().replace("__", "."));
        let anchor = format!("dthd_segment_{}", s.name);
        flags.push(format!("-Wl,--undefined={anchor}"));
        let protection = match s.prot {
            1 => "a",
            3 => "aw",
            5 => "ax",
            _ => anyhow::bail!("unsupported segment protection"),
        };
        asm.push_str(&format!(
            r#".section {section},"{protection}",@progbits
.p2align 12
.global {anchor}
{anchor}:
.incbin "dee_dthd_encoder.macho",{},{}
"#,
            s.offset, s.filesize,
        ));
        flags.push(format!("-Wl,--section-start={section}=0x{:x}", s.addr));
        if s.size > s.filesize && s.prot & 2 != 0 {
            flags.push(format!("-Wl,--undefined={anchor}_zero"));
            asm.push_str(&format!(
                ".section {section}.zero,\"aw\",@nobits\n.global {anchor}_zero\n{anchor}_zero:\n.skip {}\n",
                s.size - s.filesize
            ));
            flags.push(format!(
                "-Wl,--section-start={section}.zero=0x{:x}",
                s.addr + s.filesize
            ));
        }
    }
    asm.push_str(".section .note.GNU-stack,\"\",@progbits\n");
    fs::write(out.join("macho.S"), asm)?;
    fs::write(out.join("ldflags"), flags.join("\n") + "\n")?;

    Ok(flags)
}

pub fn textmap(input: &Path, out: &Path) -> Result<()> {
    let source = fs::read_to_string(input)?;
    let pattern =
        Regex::new(r"(?s)static const uint(16|32)_t\s+(textmap_\w+)\[\](\[2\])?\s*=\s*\{(.*?)\};")?;
    let number = Regex::new(r"0x[0-9a-fA-F]+|\d+")?;
    let mut names = std::collections::BTreeSet::new();
    let mut result = fs::File::create(out)?;

    for c in pattern.captures_iter(&source) {
        let name = c[2].to_uppercase();
        names.insert(name.clone());
        let paired = c.get(3).is_some();
        let values: Vec<_> = number.find_iter(&c[4]).map(|m| m.as_str()).collect();
        if paired {
            ensure!(values.len() % 2 == 0, "invalid textmap rows");
            writeln!(result, "pub(super) static {name}: &[[u32; 2]] = &[")?;
            for row in values.chunks_exact(2) {
                writeln!(result, "[{}, {}],", row[0], row[1])?;
            }
        } else {
            writeln!(result, "pub(super) static {name}: &[u16] = &[")?;
            for chunk in values.chunks(16) {
                writeln!(result, "{},", chunk.join(", "))?;
            }
        }
        writeln!(result, "];\n")?;
    }

    ensure!(
        names
            == [
                "TEXTMAP_POOL",
                "TEXTMAP_UPPER",
                "TEXTMAP_LOWER",
                "TEXTMAP_FOLD",
                "TEXTMAP_CASED",
                "TEXTMAP_CASE_IGNORABLE"
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        "unexpected textmap declarations"
    );
    Ok(())
}
