use anyhow::{ensure, Context, Result};
use std::{fs, path::Path, process::Command};

pub fn run(repo: &Path, directory: &Path) -> Result<()> {
    fs::create_dir_all(directory)?;
    let directory = directory.canonicalize()?;
    let lock = crate::build::lock(repo)?;

    for (name, digest) in lock["sources"].as_object().context("source pins")? {
        let target = directory.join(name);
        if target.exists() {
            ensure!(
                crate::build::hash_file(&target)? == *digest,
                "existing input has the wrong SHA-256: {}",
                target.display()
            );
            println!("verified cached {name}");
            continue;
        }

        let scratch = tempfile::Builder::new()
            .prefix(".fetch-")
            .tempdir_in(&directory)?;
        let source = scratch.path().join(name);
        let url = lock["source_urls"][name]
            .as_str()
            .with_context(|| format!("missing download URL for {name}"))?;
        let status = Command::new("curl")
            .args(["--fail", "--location", "--proto", "=https", "--output"])
            .arg(&source)
            .arg(url)
            .status()?;
        ensure!(status.success(), "source download failed: {name}");
        ensure!(
            crate::build::hash_file(&source)? == *digest,
            "download SHA-256 mismatch: {name}"
        );
        fs::rename(source, target)?;
        println!("fetched and verified {name}");
    }

    Ok(())
}
