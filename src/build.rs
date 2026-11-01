//! Compile dependencies and link the Linux executable.

use anyhow::{bail, ensure, Context, Result};
use rayon::prelude::*;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Clone, Debug)]
pub struct Options {
    pub repo: PathBuf,
    pub payload: PathBuf,
    pub sources: PathBuf,
    pub out: PathBuf,
    pub jobs: usize,
    pub verify: bool,
    pub keep: bool,
    pub allow_toolchain_drift: bool,
    pub allow_unpinned_payload: bool,
}

pub fn validate_payload(
    payload: &Path,
    pinned_hash: &str,
    capabilities: &Value,
    allow_unpinned: bool,
) -> Result<String> {
    let data = crate::generate::read_payload(payload)?;
    let digest = crate::digest(&data);
    if digest == pinned_hash {
        return Ok(digest);
    }

    ensure!(
        allow_unpinned,
        "payload does not match config/inputs.lock (use --allow-unpinned-payload)"
    );

    let report = crate::image::Image::assess(&data, capabilities, pinned_hash);
    ensure!(
        report["status"] == "candidate",
        "unpinned payload failed compatibility screening: {}",
        serde_json::to_string_pretty(&report)?
    );

    Ok(digest)
}

pub fn hash_file(path: &Path) -> Result<String> {
    use sha2::{Digest, Sha256};

    let mut file = fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65536];

    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }

    Ok(format!("{:x}", hash.finalize()))
}

pub fn lock(repo: &Path) -> Result<Value> {
    Ok(serde_json::from_slice(&fs::read(
        repo.join("config/inputs.lock"),
    )?)?)
}

fn files_under(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(root.join(dir))? {
        let entry = entry?;
        // Exclude editor files and macOS AppleDouble sidecars.
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }

        let path = dir.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            files_under(root, &path, out)?;
        } else if path.extension().is_some_and(|x| x == "rs")
            || path.file_name().is_some_and(|x| x == "Cargo.toml")
        {
            out.push(path);
        }
    }
    Ok(())
}

pub fn source_files(repo: &Path) -> Result<Vec<PathBuf>> {
    let mut files = vec![
        "Cargo.toml".into(),
        "Cargo.lock".into(),
        "rust-toolchain.toml".into(),
    ];
    files_under(repo, Path::new("src"), &mut files)?;
    files.extend([
        PathBuf::from("crates/darwin-abi/Cargo.toml"),
        PathBuf::from("crates/darwin-abi/build.rs"),
    ]);
    files_under(repo, Path::new("crates/darwin-abi/src"), &mut files)?;

    files.sort();
    Ok(files)
}

fn output(command: &mut Command) -> Result<String> {
    let result = command
        .output()
        .with_context(|| format!("run {command:?}"))?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    ensure!(
        result.status.success(),
        "command failed: {command:?}\n{text}"
    );
    Ok(text)
}

fn system(program: &str) -> Result<String> {
    let text = output(Command::new(program).arg("--version"))?;
    Ok(text.lines().next().unwrap_or("").trim().to_owned())
}

fn rustup() -> PathBuf {
    std::env::var_os("RUSTUP")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cargo/bin/rustup")))
        .filter(|p| p.is_file())
        .unwrap_or_else(|| "rustup".into())
}

fn rust_command(tool: &str) -> Command {
    let mut c = Command::new(rustup());
    c.args(["run", "1.97.1", tool]);
    c
}

fn toolchain() -> Result<BTreeMap<String, String>> {
    let mut tc = BTreeMap::new();
    for (k, program) in [
        ("gcc", "gcc"),
        ("g++", "g++"),
        ("ld", "ld"),
        ("as", "as"),
        ("libc", "ldd"),
    ] {
        tc.insert(k.into(), system(program)?);
    }

    for tool in ["rustc", "cargo"] {
        let text = output(rust_command(tool).arg("--version"))?;
        tc.insert(tool.into(), text.trim().into());
    }

    Ok(tc)
}

struct Runner<'a> {
    work: &'a Path,
    epoch: String,
}

impl Runner<'_> {
    fn run(&self, args: Vec<String>, cwd: &Path, log: Option<&Path>) -> Result<String> {
        let mut cmd = Command::new(&args[0]);
        cmd.args(&args[1..])
            .current_dir(cwd)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("LC_ALL", "C")
            .env("TZ", "UTC")
            .env("HOME", self.work)
            .env("SOURCE_DATE_EPOCH", &self.epoch);

        let result = cmd.output().with_context(|| format!("run {}", args[0]))?;
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        if let Some(log) = log {
            fs::write(log, &text)?;
        }
        ensure!(result.status.success(), "command failed: {args:?}\n{text}");
        Ok(text)
    }
}

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|v| (*v).into()).collect()
}

fn path(p: impl AsRef<Path>) -> String {
    p.as_ref().to_string_lossy().into_owned()
}

const MBSTATE: &str = r#"#ifndef ____mbstate_t_defined
#define ____mbstate_t_defined 1
typedef struct __attribute__((aligned(8))) {
    int __count;
    union {
        unsigned int __wch;
        char __wchb[4];
    } __value;
    char __darwin_padding[120];
} __mbstate_t;
#endif
"#;

fn compile(
    options: &Options,
    lock: &Value,
    work: &Path,
    version: &str,
    date: &str,
) -> Result<(PathBuf, PathBuf)> {
    let runner = Runner {
        work,
        epoch: lock["source_date_epoch"].to_string(),
    };
    let sysroot = work.join("sysroot");
    crate::sysroot::extract(&options.sources, lock, &sysroot)?;

    let root = work.join("root");
    let src = work.join("src");
    let gen = work.join("gen");
    let obj = work.join("obj");
    for d in [&root, &src, &gen, &obj.join("libcxx")] {
        fs::create_dir_all(d)?;
    }

    for (name, _) in lock["sources"].as_object().context("sources map")? {
        if name.ends_with(".deb") {
            runner.run(
                vec![
                    "dpkg-deb".into(),
                    "-x".into(),
                    path(options.sources.join(name)),
                    path(&root),
                ],
                work,
                None,
            )?;
        }
    }
    runner.run(
        vec![
            "tar".into(),
            "-xf".into(),
            path(options.sources.join("libcxx-18.1.3.src.tar.xz")),
            "-C".into(),
            path(&src),
        ],
        work,
        None,
    )?;

    let image = crate::generate::prepare(&options.payload, &gen)?;
    let ldflags = crate::generate::imports(
        &image,
        &fs::read_to_string(options.repo.join("crates/darwin-abi/src/platform/hooks.rs"))?,
        &gen,
    )?;
    crate::generate::textmap(
        &options.repo.join("data/textmap.c.inc"),
        &gen.join("textmap.rs"),
    )?;

    let llvm_inc = root.join("usr/lib/llvm-18/include/c++/v1");
    let cxx = src.join("libcxx-18.1.3.src");
    let inc = work.join("include");
    fs::create_dir_all(inc.join("bits/types"))?;
    fs::write(inc.join("bits/types/__mbstate_t.h"), MBSTATE)?;
    for h in ["cxxabi.h", "__cxxabi_config.h"] {
        fs::copy(llvm_inc.join(h), inc.join(h))?;
    }
    for header in ["__config_site", "__assertion_handler"] {
        fs::copy(llvm_inc.join(header), cxx.join("include").join(header))?;
    }

    let mut files = Vec::new();
    for dir in ["src", "src/filesystem", "src/ryu"] {
        for entry in fs::read_dir(cxx.join(dir))? {
            let p = entry?.path();
            if p.extension().is_some_and(|e| e == "cpp") && p.file_name().unwrap() != "format.cpp" {
                files.push(p);
            }
        }
    }
    files.sort();

    let prefix = [
        format!("-ffile-prefix-map={}=/dthd", work.display()),
        format!("-fdebug-prefix-map={}=/dthd", work.display()),
    ];
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(options.jobs)
        .build()?;
    let mut objects: Vec<PathBuf> = pool.install(|| {
        files
            .par_iter()
            .map(|p| -> Result<PathBuf> {
                let rel = p
                    .strip_prefix(cxx.join("src"))?
                    .components()
                    .map(|s| s.as_os_str().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("_");
                let object = obj.join("libcxx").join(format!("{rel}.o"));
                let mut cmd = args(&[
                    "g++",
                    "-std=c++20",
                    "-O2",
                    "-g0",
                    "-fPIC",
                    "-fvisibility=hidden",
                    "-fvisibility-inlines-hidden",
                    "-nostdinc++",
                    "-D_LIBCPP_BUILDING_LIBRARY",
                    "-DLIBCXX_BUILDING_LIBCXXABI",
                    "-D_LIBCPP_HAS_NO_PRAGMA_SYSTEM_HEADER",
                ]);
                cmd.extend(crate::sysroot::compile_flags(&sysroot));
                cmd.push(format!("-frandom-seed={rel}"));
                cmd.extend(prefix.clone());
                cmd.extend([
                    format!("-I{}", inc.display()),
                    format!("-I{}", cxx.join("include").display()),
                    format!("-I{}", cxx.join("src").display()),
                    "-c".into(),
                    path(p),
                    "-o".into(),
                    path(&object),
                ]);
                runner.run(cmd, work, None)?;
                Ok(object)
            })
            .collect::<Result<_>>()
    })?;
    objects.sort();

    let mut cmd = vec!["ar".into(), "rcsD".into(), path(obj.join("libc++.a"))];
    cmd.extend(objects.iter().map(path));
    runner.run(cmd, work, None)?;

    let rust = work.join("rust");
    for name in source_files(&options.repo)? {
        let target = rust.join(&name);
        fs::create_dir_all(target.parent().unwrap())?;
        fs::copy(options.repo.join(&name), target)?;
    }

    let target = work.join("cargo-target");
    let home = std::env::var_os("HOME").context("HOME is required to locate the Rust toolchain")?;
    let cargo_home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(&home).join(".cargo"));
    let rustup_home = std::env::var_os("RUSTUP_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(&home).join(".rustup"));
    let mut cargo = rust_command("cargo");
    cargo
        .env_clear()
        .env("HOME", &home)
        .env("CARGO_HOME", &cargo_home)
        .env("RUSTUP_HOME", rustup_home)
        .env("PATH", "/usr/bin:/bin");
    cargo.args([
        "build",
        "--release",
        "--locked",
        "--offline",
        "-p",
        "dthd-darwin-abi",
        "--features",
        "embedded",
    ]);
    cargo
        .current_dir(&rust)
        .env("DTHD_GENERATED", &gen)
        .env("DTHD_BUILDER_NAME", env!("CARGO_PKG_NAME"))
        .env("DTHD_PORT_VERSION", version)
        .env("DTHD_PORT_DATE", date)
        .env("CARGO_TARGET_DIR", &target)
        .env("CARGO_INCREMENTAL", "0")
        .env("LC_ALL", "C")
        .env("TZ", "UTC")
        .env("SOURCE_DATE_EPOCH", &runner.epoch)
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_ENCODED_RUSTFLAGS");
    cargo.env(
        "CARGO_ENCODED_RUSTFLAGS",
        [
            format!("--remap-path-prefix={}=/dthd", work.display()),
            format!("--remap-path-prefix={}=/cargo", cargo_home.display()),
            "-Cforce-unwind-tables=yes".into(),
            "-Crelocation-model=static".into(),
            "-Ctarget-cpu=x86-64".into(),
            "-Cllvm-args=-fp-contract=off".into(),
        ]
        .join("\x1f"),
    );

    let build_output = output(&mut cargo)?;
    fs::write(work.join("runtime.log"), build_output)?;

    for name in ["imports", "macho"] {
        runner.run(
            vec![
                "gcc".into(),
                "-c".into(),
                format!("{name}.S"),
                "-o".into(),
                path(obj.join(format!("{name}.o"))),
            ],
            &gen,
            None,
        )?;
    }

    let lib = root.join("usr/lib/llvm-18/lib");
    let sym = work.join("dee_dthd_encoder.sym");
    let stripped = work.join("dee_dthd_encoder");
    let mut cmd = vec![
        "gcc".into(),
        "-no-pie".into(),
        "-o".into(),
        path(&sym),
        path(obj.join("imports.o")),
        path(obj.join("macho.o")),
        path(target.join("release/libdthd_darwin_abi.a")),
        path(obj.join("libc++.a")),
        path(lib.join("libc++abi.a")),
        path(lib.join("libunwind.a")),
    ];
    cmd.extend(crate::sysroot::link_flags(&sysroot));
    cmd.extend(args(&[
        "-static-libgcc",
        "-Wl,--exclude-libs,ALL",
        "-Wl,--wrap=dl_iterate_phdr",
        "-Wl,--wrap=fwrite",
        "-Wl,--disable-new-dtags,-rpath,$ORIGIN/",
        "-Wl,--build-id=sha1",
        "-Wl,--gc-sections",
        "-ldl",
        "-lpthread",
        "-lm",
    ]));
    cmd.extend(ldflags);
    runner.run(cmd, work, Some(&work.join("link.log")))?;
    runner.run(
        vec!["strip".into(), "-o".into(), path(&stripped), path(&sym)],
        work,
        None,
    )?;

    Ok((stripped, sym))
}

fn shape(elf: &Path, lock: &Value) -> Result<Value> {
    let hdr = output(Command::new("readelf").args(["-hdlW"]).arg(elf))?;
    ensure!(
        hdr.contains("EXEC (Executable file)"),
        "not a non-PIE executable"
    );

    let needed: Vec<_> = regex::Regex::new(r"\(NEEDED\)\s+Shared library: \[([^\]]+)\]")?
        .captures_iter(&hdr)
        .map(|c| c[1].to_owned())
        .collect();
    for n in &needed {
        ensure!(
            lock["shape"]["needed_allowed"]
                .as_array()
                .context("needed_allowed")?
                .iter()
                .any(|v| v == n),
            "unexpected dependency: {n}"
        );
    }
    ensure!(
        hdr.contains(&format!(
            "Library rpath: [{}]",
            lock["shape"]["rpath"].as_str().unwrap_or("")
        )),
        "unexpected RPATH"
    );
    ensure!(
        regex::Regex::new(r"LOAD\s+0x[0-9a-f]+ 0x0*100000000 ")?.is_match(&hdr),
        "no Mach-O PT_LOAD"
    );

    let glibc_max = check_glibc(
        elf,
        lock["shape"]["glibc_ceiling"]
            .as_str()
            .context("glibc ceiling")?,
    )?;
    let dyns = output(Command::new("objdump").arg("-T").arg(elf))?;
    let runtime = regex::Regex::new(r"\s(_Z|__cxa|_Unwind)")?;
    ensure!(
        !dyns
            .lines()
            .any(|l| runtime.is_match(l) && !l.contains(" *UND*")),
        "exports C++ runtime symbols"
    );

    Ok(json!({ "needed": needed, "glibc_max": glibc_max }))
}

pub fn run(mut options: Options) -> Result<Value> {
    ensure!(
        cfg!(all(target_os = "linux", target_arch = "x86_64")),
        "ELF builds require Linux x86-64"
    );
    ensure!(options.jobs > 0, "--jobs must be positive");

    options.repo = options.repo.canonicalize()?;
    let (version, date) = crate::version::describe(&options.repo)?;
    options.payload = options.payload.canonicalize()?;
    options.sources = options.sources.canonicalize()?;

    let lock = lock(&options.repo)?;
    let capabilities =
        serde_json::from_slice(&fs::read(options.repo.join("config/compatibility.json"))?)?;
    let pinned_payload = lock["payload"]["sha256"].as_str().context("payload hash")?;
    let payload_sha256 = validate_payload(
        &options.payload,
        pinned_payload,
        &capabilities,
        options.allow_unpinned_payload,
    )?;
    crate::textmap::snapshot(&options.repo)?;
    crate::sysroot::verify_sources(&options.sources, &lock)?;

    let tc = toolchain()?;
    let drift: BTreeMap<_, _> = tc
        .iter()
        .filter(|(k, v)| lock["toolchain"][*k] != v.as_str())
        .collect();
    ensure!(
        drift.is_empty() || options.allow_toolchain_drift,
        "toolchain differs from lock: {drift:?} (use --allow-toolchain-drift)"
    );

    output(
        rust_command("cargo")
            .args(["fetch", "--locked", "--manifest-path"])
            .arg(options.repo.join("Cargo.toml")),
    )?;

    let mut works = Vec::new();
    let mut results = Vec::new();
    for n in 0..if options.verify { 2 } else { 1 } {
        let w = tempfile::Builder::new()
            .prefix(&format!(
                "{}-{n}-{}-",
                env!("CARGO_PKG_NAME"),
                "x".repeat(n * 7)
            ))
            .tempdir()?;
        println!("build {} in {}", n + 1, w.path().display());
        let result = compile(&options, &lock, w.path(), &version, &date);
        match result {
            Ok((exe, sym)) => {
                let hashes = (hash_file(&exe)?, hash_file(&sym)?);
                println!("build {}: {}", n + 1, hashes.0);
                results.push((exe, sym, hashes));
                works.push(w);
            }
            Err(error) => {
                let kept = w.keep();
                return Err(error.context(format!("build directory retained: {}", kept.display())));
            }
        }
    }

    if options.verify && results[0].2 != results[1].2 {
        let kept: Vec<_> = works.into_iter().map(|w| w.keep()).collect();
        bail!("NOT REPRODUCIBLE; directories retained: {kept:?}");
    }

    let (exe, sym, (digest, sym_digest)) = &results[0];
    let shape = shape(exe, &lock)?;
    fs::create_dir_all(&options.out)?;
    fs::copy(exe, options.out.join("dee_dthd_encoder"))?;
    fs::copy(sym, options.out.join("dee_dthd_encoder.sym"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for name in ["dee_dthd_encoder", "dee_dthd_encoder.sym"] {
            fs::set_permissions(options.out.join(name), fs::Permissions::from_mode(0o755))?;
        }
    }
    fs::write(
        options.out.join("SHA256SUMS"),
        format!("{digest}  dee_dthd_encoder\n{sym_digest}  dee_dthd_encoder.sym\n"),
    )?;

    let report = json!({
        "output_sha256": digest,
        "sym_sha256": sym_digest,
        "payload_sha256": payload_sha256,
        "payload_matches_pin": payload_sha256 == pinned_payload,
        "sources": lock["sources"],
        "sysroot": lock["sysroot"],
        "generated_inputs": lock["generated_inputs"],
        "toolchain": tc,
        "toolchain_drift": drift,
        "shape": shape,
        "port_version": format!("{version} {date}"),
        "verified_twice": options.verify,
        "fft": "own-rust",
    });
    fs::write(
        options.out.join("BUILDINFO.json"),
        format!("{}\n", serde_json::to_string_pretty(&report)?),
    )?;

    if options.keep {
        for w in works {
            println!("retained {}", w.keep().display());
        }
    }
    println!(
        "{digest}  {}",
        options.out.join("dee_dthd_encoder").display()
    );

    Ok(report)
}

fn check_glibc(elf: &Path, ceiling: &str) -> Result<Option<String>> {
    let symbols = output(Command::new("objdump").arg("-T").arg(elf))?;
    let versions: std::collections::BTreeSet<Vec<u32>> =
        regex::Regex::new(r"GLIBC_(\d+\.\d+(?:\.\d+)?)")?
            .captures_iter(&symbols)
            .map(|c| c[1].split('.').map(|s| s.parse().unwrap()).collect())
            .collect();
    let limit: Vec<u32> = ceiling
        .split('.')
        .map(str::parse)
        .collect::<Result<_, _>>()?;

    if let Some(max) = versions.last() {
        ensure!(
            max <= &limit,
            "GLIBC version exceeds ceiling {ceiling}: {max:?}"
        );
    }

    Ok(versions.last().map(|version| {
        version
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(".")
    }))
}

pub fn builder(repo: &Path, sources: &Path, out: &Path) -> Result<()> {
    ensure!(
        cfg!(all(target_os = "linux", target_arch = "x86_64")),
        "builder release builds require Linux x86-64"
    );

    let repo = repo.canonicalize()?;
    let sources = sources.canonicalize()?;
    let lock = lock(&repo)?;
    crate::sysroot::verify_sources(&sources, &lock)?;

    let work = tempfile::Builder::new().prefix("dthd-builder-").tempdir()?;
    let root = work.path().join("sysroot");
    crate::sysroot::extract(&sources, &lock, &root)?;
    let target = work.path().join("target");
    let flags = crate::sysroot::link_flags(&root)
        .into_iter()
        .map(|flag| format!("-Clink-arg={flag}"))
        .chain([format!("--remap-path-prefix={}=/dthd", repo.display())])
        .collect::<Vec<_>>();

    output(
        rust_command("cargo")
            .args([
                "build",
                "--locked",
                "--release",
                "--bin",
                env!("CARGO_PKG_NAME"),
                "--target",
                "x86_64-unknown-linux-gnu",
            ])
            .current_dir(&repo)
            .env("CARGO_TARGET_DIR", &target)
            .env_remove("RUSTFLAGS")
            .env("CARGO_ENCODED_RUSTFLAGS", flags.join("\x1f")),
    )?;

    let binary = target
        .join("x86_64-unknown-linux-gnu/release")
        .join(env!("CARGO_PKG_NAME"));
    let glibc = check_glibc(
        &binary,
        lock["shape"]["glibc_ceiling"]
            .as_str()
            .context("glibc ceiling")?,
    )?;
    crate::sysroot::smoke_test(&root, &binary)?;

    fs::create_dir_all(out)?;
    let destination = out.join(env!("CARGO_PKG_NAME"));
    fs::copy(&binary, &destination)?;
    println!(
        "{} (GLIBC {})",
        destination.display(),
        glibc.as_deref().unwrap_or("none")
    );
    Ok(())
}
