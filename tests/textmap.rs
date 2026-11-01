use dthd_mac2linux::{generate, textmap};
use std::{fs, path::Path};

#[test]
fn bundled_snapshot_matches_pin_and_generates_runtime_tables() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    let input = textmap::snapshot(repo).unwrap();
    let out = tempfile::tempdir().unwrap();
    let output = out.path().join("textmap.rs");
    generate::textmap(&input, &output).unwrap();

    let tables = fs::read_to_string(output).unwrap();
    assert!(tables.contains("TEXTMAP_POOL: &[u16]"));
    assert!(tables.contains("TEXTMAP_CASE_IGNORABLE: &[[u32; 2]]"));
    assert!(tables.contains("[0x61, 0x1]")); // ASCII a -> A, at pool offset zero.
}

#[test]
fn changed_snapshot_is_rejected() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    let out = tempfile::tempdir().unwrap();
    fs::create_dir(out.path().join("config")).unwrap();
    fs::create_dir(out.path().join("data")).unwrap();
    fs::copy(
        repo.join("config/inputs.lock"),
        out.path().join("config/inputs.lock"),
    )
    .unwrap();
    fs::write(out.path().join("data/textmap.c.inc"), "changed").unwrap();

    let error = textmap::snapshot(out.path()).unwrap_err();
    assert!(error.to_string().contains("does not match"));
}
