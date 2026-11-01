use std::{env, error::Error, fs, path::PathBuf};

const EMPTY_IMAGE: &str = r#"
pub(crate) static SEGMENTS: &[Segment] = &[];
pub(crate) static BINDINGS: &[Binding] = &[];
pub(crate) static INITIALIZERS: &[usize] = &[];
pub(crate) const ENTRY: usize = 0;
pub(crate) static COMPACT: &[Compact] = &[];
pub(crate) static PERSONALITIES: &[u32] = &[];
"#;

const EMPTY_TEXTMAP: &str = r#"
pub(super) static TEXTMAP_POOL: &[u16] = &[];
pub(super) static TEXTMAP_UPPER: &[[u32; 2]] = &[];
pub(super) static TEXTMAP_LOWER: &[[u32; 2]] = &[];
pub(super) static TEXTMAP_FOLD: &[[u32; 2]] = &[];
pub(super) static TEXTMAP_CASED: &[[u32; 2]] = &[];
pub(super) static TEXTMAP_CASE_IGNORABLE: &[[u32; 2]] = &[];
"#;

fn main() -> Result<(), Box<dyn Error>> {
    for name in [
        "DTHD_GENERATED",
        "DTHD_BUILDER_NAME",
        "DTHD_PORT_VERSION",
        "DTHD_PORT_DATE",
    ] {
        println!("cargo:rerun-if-env-changed={name}");
    }

    for (name, fallback) in [
        ("DTHD_BUILDER_NAME", env!("CARGO_PKG_NAME")),
        ("DTHD_PORT_VERSION", env!("CARGO_PKG_VERSION")),
        ("DTHD_PORT_DATE", "unreleased"),
    ] {
        let value = env::var(name).unwrap_or_else(|_| fallback.to_owned());
        println!("cargo:rustc-env={name}={value}");
    }

    let output = PathBuf::from(env::var("OUT_DIR")?);
    if let Some(directory) = env::var_os("DTHD_GENERATED") {
        let directory = PathBuf::from(directory);
        for name in ["image.rs", "textmap.rs"] {
            let input = directory.join(name);
            println!("cargo:rerun-if-changed={}", input.display());
            fs::copy(input, output.join(name))?;
        }
    } else {
        if env::var_os("CARGO_FEATURE_EMBEDDED").is_some() {
            return Err("embedded runtime requires DTHD_GENERATED".into());
        }
        fs::write(output.join("image.rs"), EMPTY_IMAGE)?;
        fs::write(output.join("textmap.rs"), EMPTY_TEXTMAP)?;
    }

    Ok(())
}
