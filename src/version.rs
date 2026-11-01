//! Encoder version from the nearest annotated release tag.

use anyhow::{ensure, Context, Result};
use std::{path::Path, process::Command};

fn git(repo: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .context("run Git to identify the encoder build")?;
    ensure!(
        output.status.success(),
        "git {}: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

pub fn describe(repo: &Path) -> Result<(String, String)> {
    let root = git(repo, &["rev-parse", "--show-toplevel"])
        .context("build from a Git checkout with its release tags")?;
    ensure!(
        Path::new(&root).canonicalize()? == repo.canonicalize()?,
        "builder directory must be the root of its Git checkout"
    );

    let commit = git(repo, &["rev-parse", "HEAD"])?;
    let abbreviated = git(repo, &["rev-parse", "--short=7", &commit])?;
    let description = git(repo, &["describe", "--long", "--match", "v[0-9]*", &commit])
        .context("no reachable annotated release tag; fetch the release tags and history")?;
    let (prefix, _) = description.rsplit_once("-g").context("Git commit suffix")?;
    let (tag, count) = prefix.rsplit_once('-').context("Git commit count")?;
    let count: u64 = count.parse()?;
    let version = semver::Version::parse(tag.strip_prefix('v').context("release tag prefix")?)?;
    ensure!(
        tag == format!("v{version}") && version.build.is_empty(),
        "release tag must be vMAJOR.MINOR.PATCH, optionally with a prerelease suffix"
    );

    let mut label = if count == 0 {
        format!("{tag}-g{abbreviated}")
    } else {
        format!("{tag}+{count}-g{abbreviated}")
    };
    if !git(repo, &["status", "--porcelain", "--untracked-files=no"])?.is_empty() {
        label.push_str("-dirty");
    }
    let date = git(repo, &["show", "-s", "--format=%cs", &commit])?;

    Ok((label, date))
}
