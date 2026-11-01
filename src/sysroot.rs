//! Extract the pinned Linux headers and libraries without installing them.

use anyhow::{ensure, Context, Result};
use serde_json::Value;
use std::{
    fs,
    path::Path,
    process::{Command, Stdio},
};

pub fn verify_sources(sources: &Path, lock: &Value) -> Result<()> {
    for (name, hash) in lock["sources"].as_object().context("source pins")? {
        ensure!(
            crate::build::hash_file(&sources.join(name))? == *hash,
            "source hash mismatch: {name}"
        );
    }

    Ok(())
}

pub fn extract(sources: &Path, lock: &Value, root: &Path) -> Result<()> {
    fs::create_dir_all(root.join("usr/lib64"))?;
    #[cfg(unix)]
    if !root.join("lib64").try_exists()? {
        std::os::unix::fs::symlink("usr/lib64", root.join("lib64"))?;
    }

    for package in lock["sysroot"]["packages"]
        .as_array()
        .context("sysroot packages")?
    {
        let name = package.as_str().context("sysroot package name")?;
        let mut rpm = Command::new("rpm2cpio")
            .arg(sources.join(name))
            .stdout(Stdio::piped())
            .spawn()
            .context("run rpm2cpio")?;
        let unpack = Command::new("cpio")
            .args(["-idmu", "--quiet", "--no-absolute-filenames"])
            .current_dir(root)
            .stdin(rpm.stdout.take().context("rpm2cpio output")?)
            .output();
        let status = rpm.wait()?;
        let unpack = unpack.context("run cpio")?;
        ensure!(
            status.success() && unpack.status.success(),
            "extract {name}: {}",
            String::from_utf8_lossy(&unpack.stderr)
        );
    }

    Ok(())
}

pub fn compile_flags(root: &Path) -> Vec<String> {
    vec![
        format!("--sysroot={}", root.display()),
        format!("-B{}/usr/lib64/", root.display()),
    ]
}

pub fn link_flags(root: &Path) -> Vec<String> {
    let mut flags = compile_flags(root);
    flags.extend([
        format!("-L{}/usr/lib64", root.display()),
        format!("-L{}/lib64", root.display()),
        format!("-Wl,--rpath-link,{}/lib64", root.display()),
        format!("-Wl,--rpath-link,{}/usr/lib64", root.display()),
    ]);

    flags
}

pub fn smoke_test(root: &Path, binary: &Path) -> Result<()> {
    let output = Command::new(root.join("lib64/ld-linux-x86-64.so.2"))
        .arg("--library-path")
        .arg(format!("{0}/lib64:{0}/usr/lib64", root.display()))
        .arg(binary)
        .arg("--version")
        .output()?;
    ensure!(
        output.status.success(),
        "builder failed with the pinned glibc runtime: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    Ok(())
}
