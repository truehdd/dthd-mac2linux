use dthd_mac2linux::image::{Image, BASE};
use dthd_mac2linux::{build, generate};
use serde_json::json;

fn u32s(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

fn put32(b: &mut [u8], at: usize, v: u32) {
    b[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

fn put64(b: &mut [u8], at: usize, v: u64) {
    b[at..at + 8].copy_from_slice(&v.to_le_bytes());
}

fn label(b: &mut [u8], at: usize, s: &str) {
    b[at..at + s.len()].copy_from_slice(s.as_bytes());
}

fn section(name: &str, owner: &str, addr: u64, size: u64, offset: u32, flags: u32) -> Vec<u8> {
    let mut b = vec![0; 80];
    label(&mut b, 0, name);
    label(&mut b, 16, owner);
    put64(&mut b, 32, addr);
    put64(&mut b, 40, size);
    put32(&mut b, 48, offset);
    put32(&mut b, 64, flags);
    b
}

fn segment(
    name: &str,
    addr: u64,
    size: u64,
    offset: u64,
    file_size: u64,
    prot: u32,
    sections: Vec<u8>,
) -> Vec<u8> {
    let mut b = vec![0; 72];
    put32(&mut b, 0, 0x19);
    put32(&mut b, 4, 72 + sections.len() as u32);
    label(&mut b, 8, name);
    put64(&mut b, 24, addr);
    put64(&mut b, 32, size);
    put64(&mut b, 40, offset);
    put64(&mut b, 48, file_size);
    put32(&mut b, 56, prot);
    put32(&mut b, 60, prot);
    put32(&mut b, 64, sections.len() as u32 / 80);
    b.extend(sections);
    b
}

fn fixture(binding: &[u8], extra: &[u8]) -> Vec<u8> {
    let mut main = u32s(&[0x80000028, 24]);
    main.extend(0x700u64.to_le_bytes());
    main.extend(0u64.to_le_bytes());

    let mut commands = vec![
        segment("__PAGEZERO", 0, BASE, 0, 0, 0, vec![]),
        segment(
            "__TEXT",
            BASE,
            4096,
            0,
            4096,
            5,
            section("__unwind_info", "__TEXT", BASE + 0x600, 0x100, 0x600, 0),
        ),
        segment(
            "__DATA",
            BASE + 0x1000,
            4096,
            0x1000,
            4096,
            3,
            section("__mod_init_func", "__DATA", BASE + 0x1100, 8, 0x1100, 9),
        ),
        segment("__LINKEDIT", BASE + 0x2000, 4096, 0x2000, 4096, 1, vec![]),
        u32s(&[
            0x80000022,
            48,
            0,
            0,
            0x2200,
            binding.len() as u32,
            0,
            0,
            0,
            0,
            0,
            0,
        ]),
        main,
    ];
    if !extra.is_empty() {
        commands.push(extra.to_vec());
    }

    let mut header = u32s(&[
        0xfeedfacf,
        0x1000007,
        3,
        2,
        commands.len() as u32,
        commands.iter().map(|v| v.len() as u32).sum(),
        0,
        0,
    ]);
    for c in commands {
        header.extend(c);
    }

    let mut b = vec![0; 0x3000];
    b[..header.len()].copy_from_slice(&header);

    let mut unwind = u32s(&[1, 0, 0, 0, 0, 28, 2, 0x700, 52, 52, 0x780, 0, 52]);
    unwind.extend(u32s(&[2]));
    unwind.extend(8u16.to_le_bytes());
    unwind.extend(1u16.to_le_bytes());
    unwind.extend(u32s(&[0x700, 0x01000000]));
    b[0x600..0x600 + unwind.len()].copy_from_slice(&unwind);
    put64(&mut b, 0x1100, BASE + 0x700);
    b[0x2200..0x2200 + binding.len()].copy_from_slice(binding);
    b
}

const BIND: &[u8] = b"\x40_malloc\0\x51\x72\0\x90\0";

fn report(b: &[u8]) -> serde_json::Value {
    Image::assess(b, &json!({ "imports": ["_malloc"], "dylibs": [] }), "")
}

#[test]
fn candidate_is_not_a_pin() {
    let r = report(&fixture(BIND, &[]));
    assert_eq!(r["status"], "candidate");
    assert_eq!(r["matches_payload_pin"], false);
    assert_eq!(
        r["counts"],
        json!({ "imports": 1, "bindings": 1, "unwind_records": 1 })
    );
}

#[test]
fn unpinned_build_requires_opt_in_and_records_actual_hash() {
    let data = fixture(BIND, &[]);
    let root = tempfile::tempdir().unwrap();
    let payload = root.path().join("encoder");
    std::fs::write(&payload, &data).unwrap();
    let capabilities = json!({ "imports": ["_malloc"], "dylibs": [] });
    let pinned_hash = "0".repeat(64);

    let error = build::validate_payload(&payload, &pinned_hash, &capabilities, false).unwrap_err();
    assert!(error.to_string().contains("--allow-unpinned-payload"));

    let actual = build::validate_payload(&payload, &pinned_hash, &capabilities, true).unwrap();
    assert_eq!(actual, dthd_mac2linux::digest(&data));
    assert_ne!(actual, pinned_hash);
    assert_eq!(
        build::validate_payload(&payload, &actual, &capabilities, false).unwrap(),
        actual
    );
}

#[test]
fn unpinned_build_still_rejects_unsupported_or_unknown_features() {
    let root = tempfile::tempdir().unwrap();
    let payload = root.path().join("encoder");
    let capabilities = json!({ "imports": ["_malloc"], "dylibs": [] });

    for data in [
        b"bad".to_vec(),
        fixture(b"\x40_unknown\0\x72\0\x90", &[]),
        fixture(BIND, &u32s(&[0x1234, 8])),
        fixture(BIND, &u32s(&[0x80000034, 16, 0, 0])),
    ] {
        std::fs::write(&payload, data).unwrap();
        let error = build::validate_payload(&payload, "", &capabilities, true).unwrap_err();
        assert!(error.to_string().contains("failed compatibility screening"));
    }
}

#[test]
fn unknown_symbols_and_commands_need_review() {
    let r = report(&fixture(b"\x40_unknown\0\x72\0\x90", &[]));
    assert_eq!(r["status"], "review");
    assert_eq!(r["unknown_imports"], json!(["_unknown"]));
    assert_eq!(
        report(&fixture(BIND, &u32s(&[0x1234, 8])))["status"],
        "review"
    );
}

#[test]
fn unsupported_loader_features_are_rejected() {
    for extra in [u32s(&[0x80000034, 16, 0, 0]), u32s(&[0x21, 16, 0, 0])] {
        assert_eq!(report(&fixture(BIND, &extra))["status"], "reject");
    }

    let mut b = fixture(BIND, &[]);
    put32(&mut b, 24, 0x20000);
    assert_eq!(report(&b)["status"], "reject");
}

#[test]
fn bounds_and_header_rejections() {
    let b = fixture(BIND, &[]);
    for size in [0, 4, 31, 32, 80, 0x300, 0x600, b.len() - 1] {
        assert_eq!(report(&b[..size])["status"], "reject");
    }

    for (at, v) in [
        (0, 0xcafebabe),
        (4, 0x100000c),
        (12, 6),
        (16, u32::MAX),
        (36, 0),
    ] {
        let mut b = b.clone();
        put32(&mut b, at, v);
        assert_eq!(report(&b)["status"], "reject");
    }
}

#[test]
fn malformed_binding_streams_are_bounded() {
    let mut streams = vec![
        b"\x40_malloc\0\x52\x72\0\x90".to_vec(),
        b"\x40_malloc\0\x51\x7f\0\x90".to_vec(),
        b"\x40_malloc\0\x51\x72\xfd\x1f\x90".to_vec(),
        b"\xd0".to_vec(),
        b"\x40unterminated".to_vec(),
        b"\x20\x80".to_vec(),
        b"\xc0\xff\xff\xff\xff\x7f\0".to_vec(),
    ];
    let mut huge = vec![0x20];
    huge.extend([0x80; 20]);
    streams.push(huge);

    for s in streams {
        assert_eq!(report(&fixture(&s, &[]))["status"], "reject", "{s:x?}");
    }
}

#[test]
fn wrapped_uleb_delta_moves_backwards() {
    let mut delta = u64::MAX - 15;
    let mut b = b"\x40_malloc\0\x51\x72\x08\x90\x80".to_vec();
    while delta >= 128 {
        b.push((delta as u8 & 127) | 128);
        delta >>= 7;
    }
    b.push(delta as u8);
    b.extend(b"\x90\0");
    assert_eq!(report(&fixture(&b, &[]))["status"], "candidate");
}

#[test]
fn initializer_and_runtime_sections_are_validated() {
    let mut b = fixture(BIND, &[]);
    put64(&mut b, 0x1100, BASE + 0x1000);
    assert_eq!(report(&b)["status"], "reject");

    let section_at = 32 + 72 + 72;
    for kind in [1, 0x13] {
        let mut b = fixture(BIND, &[]);
        put32(&mut b, section_at + 64, kind);
        assert_eq!(report(&b)["status"], "reject");
    }
}

#[test]
fn readonly_tail_and_unwind_modes_are_rejected() {
    let mut b = fixture(BIND, &[]);
    let linkedit = 32 + 72 + 152 + 152;
    put64(&mut b, linkedit + 32, 8192);
    assert_eq!(report(&b)["status"], "reject");

    let mut b = fixture(BIND, &[]);
    put32(&mut b, 0x600 + 64, 0x02000000);
    assert_eq!(report(&b)["status"], "reject");
}

#[test]
fn compressed_unwind_pages() {
    let mut b = fixture(BIND, &[]);
    let p = 0x600 + 52;
    put32(&mut b, p, 3);
    b[p + 4..p + 12].copy_from_slice(&[12, 0, 1, 0, 16, 0, 1, 0]);
    put32(&mut b, p + 12, 0);
    put32(&mut b, p + 16, 0x01000000);
    assert_eq!(report(&b)["status"], "candidate");

    put32(&mut b, p + 12, 0xff000000);
    assert_eq!(report(&b)["status"], "reject");
}

#[test]
fn generated_imports_exclude_adapted_symbols() {
    let mut image = Image::default();
    image.parse(&fixture(BIND, &[])).unwrap();

    let out = tempfile::tempdir().unwrap();
    let flags =
        generate::imports(&image, "b\"malloc\"\n => malloc as *const (),", out.path()).unwrap();

    let asm = std::fs::read_to_string(out.path().join("imports.S")).unwrap();
    assert!(asm.contains(".quad 0"));
    assert!(!asm.contains(",malloc"));
    assert!(flags
        .iter()
        .any(|s| s == "-Wl,--section-start=.dthd.text=0x100000000"));
}

#[test]
fn cli_reports_exit_status_and_json() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));

    let out = tempfile::tempdir().unwrap();
    let input = out.path().join("input");
    for (data, status, code) in [
        (fixture(BIND, &[]), "candidate", 0),
        (fixture(b"\x40_unknown\0\x72\0\x90", &[]), "review", 2),
        (b"bad".to_vec(), "reject", 1),
    ] {
        std::fs::write(&input, data).unwrap();
        let run = std::process::Command::new(env!("CARGO_BIN_EXE_dthd-mac2linux"))
            .arg("--repo")
            .arg(root)
            .args(["check-payload", "--json"])
            .arg(&input)
            .output()
            .unwrap();

        assert_eq!(run.status.code(), Some(code));
        let value: serde_json::Value = serde_json::from_slice(&run.stdout).unwrap();
        assert_eq!(value["status"], status);
    }
}

#[test]
fn source_files_exclude_os_metadata() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("src/.cache")).unwrap();
    std::fs::create_dir_all(root.path().join("crates/darwin-abi/src")).unwrap();
    for name in [
        "src/lib.rs",
        "src/._lib.rs",
        "src/.cache/unused.rs",
        "crates/darwin-abi/src/lib.rs",
        "crates/darwin-abi/src/._lib.rs",
    ] {
        std::fs::write(root.path().join(name), "").unwrap();
    }

    let files = build::source_files(root.path()).unwrap();
    assert!(files.contains(&"src/lib.rs".into()));
    assert!(files.iter().all(|p| p
        .components()
        .all(|c| !c.as_os_str().to_string_lossy().starts_with('.'))));
}
