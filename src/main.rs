use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use dthd_mac2linux::{build, generate, image::Image};
use std::{fs, path::PathBuf, process::ExitCode};

#[derive(Parser)]
#[command(
    version,
    about = "Build and inspect the macOS x86-64 dee_dthd_encoder payload"
)]
struct Cli {
    #[arg(
        long,
        global = true,
        default_value = ".",
        help = "Root of the builder source checkout"
    )]
    repo: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Build the Linux encoder (Linux x86-64).
    Build(BuildArgs),
    /// Build the Linux builder executable against the pinned glibc sysroot.
    BuildBuilder {
        #[arg(long)]
        sources: PathBuf,
        #[arg(long, default_value = "dist")]
        out: PathBuf,
    },
    /// Screen metadata without executing the input; exit 0=candidate, 1=reject, 2=review.
    CheckPayload {
        payload: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Generate Rust metadata and assembly for inspection.
    Prepare {
        payload: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Download and verify the pinned public source archives.
    FetchSources { directory: PathBuf },
    /// Regenerate and verify the bundled Unicode snapshot (macOS).
    Textmap { output: PathBuf },
}

#[derive(Args)]
struct BuildArgs {
    #[arg(long)]
    payload: PathBuf,
    #[arg(long)]
    sources: PathBuf,
    #[arg(long, default_value = "dist")]
    out: PathBuf,
    #[arg(long, default_value_t = 6)]
    jobs: usize,
    #[arg(long)]
    verify: bool,
    #[arg(long)]
    keep: bool,
    #[arg(long)]
    allow_toolchain_drift: bool,
    /// Allow a different payload hash if compatibility screening passes.
    #[arg(long)]
    allow_unpinned_payload: bool,
}

fn run(cli: Cli) -> Result<u8> {
    match cli.command {
        Command::Build(a) => {
            build::run(build::Options {
                repo: cli.repo,
                payload: a.payload,
                sources: a.sources,
                out: a.out,
                jobs: a.jobs,
                verify: a.verify,
                keep: a.keep,
                allow_toolchain_drift: a.allow_toolchain_drift,
                allow_unpinned_payload: a.allow_unpinned_payload,
            })?;
        }
        Command::BuildBuilder { sources, out } => {
            build::builder(&cli.repo, &sources, &out)?;
        }
        Command::CheckPayload { payload, json } => {
            let lock = build::lock(&cli.repo)?;
            let cap =
                serde_json::from_slice(&fs::read(cli.repo.join("config/compatibility.json"))?)?;
            let report = match generate::read_payload(&payload) {
                Ok(data) => Image::assess(
                    &data,
                    &cap,
                    lock["payload"]["sha256"].as_str().context("payload hash")?,
                ),
                Err(e) => serde_json::json!({ "status": "reject", "blockers": [e.to_string()] }),
            };

            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!(
                    "{}: {}",
                    report["status"].as_str().unwrap().to_uppercase(),
                    payload.display()
                );
                for key in ["blockers", "review", "unknown_imports", "unknown_dylibs"] {
                    for item in report[key].as_array().into_iter().flatten() {
                        println!("  {key}: {}", item.as_str().unwrap_or(""));
                    }
                }
                if let Some(pin) = report.get("matches_payload_pin") {
                    println!("Payload hash matches build lock: {pin}");
                }
                println!(
                    "Candidate means eligible for evaluation; encoding parity still needs testing."
                );
            }

            return Ok(match report["status"].as_str() {
                Some("candidate") => 0,
                Some("review") => 2,
                _ => 1,
            });
        }
        Command::Prepare { payload, out } => {
            let image = generate::prepare(&payload, &out)?;
            generate::imports(
                &image,
                &fs::read_to_string(cli.repo.join("crates/darwin-abi/src/platform/hooks.rs"))?,
                &out,
            )?;
            let input = dthd_mac2linux::textmap::snapshot(&cli.repo)?;
            generate::textmap(&input, &out.join("textmap.rs"))?;
            println!(
                "{} bindings, {} unwind records",
                image.bindings.len(),
                image.compact.len()
            );
        }
        Command::FetchSources { directory } => dthd_mac2linux::fetch::run(&cli.repo, &directory)?,
        Command::Textmap { output } => dthd_mac2linux::textmap::run(&cli.repo, &output)?,
    }

    Ok(0)
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("{}: {error:#}", env!("CARGO_BIN_NAME"));
            ExitCode::FAILURE
        }
    }
}
