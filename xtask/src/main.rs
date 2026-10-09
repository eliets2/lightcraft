//! Workspace tooling: `cargo xtask <command>`.
//!
//! Pure Rust (std + serde_json; flate2/brotli for the web bundle). External tools (`cargo`, `curl`, `tar`) are
//! invoked through `std::process::Command`.

mod assets;
mod bench;
mod ico;
mod layers;
mod parity;
mod stats;
mod version;
mod web;

use std::path::PathBuf;
use std::process::{Command, ExitCode};

const USAGE: &str = "\
usage: cargo xtask <command>

commands:
  assets          every image/icon/font/media file is attributed in assets/ATTRIBUTION.md; no Adobe assets
  bench [FILE] [--strict] [--threshold PCT]
                  run the render benchmark, append to target/bench/history.jsonl, compare CPU time with
                  the previous run (default input: corpus/raw/arw-sony-a7m3-compressed.arw)
  ico <out.ico> <in.png>...
                  pack square PNGs (<= 256 px) into a Windows .ico (see packaging/icons.sh)
  layers          enforce the crate dependency layering (plan/architecture.md §3)
  parity [--write]
                  check docs/parity.md (every cmd:/ctl: id and path it cites exists) and print the
                  Lightroom parity summary; --write refreshes the summary table in the document
  wasm            cargo check --target wasm32-unknown-unknown for the wasm-safe crates (+ the web app)
  web [--serve [port]] [--dev]
                  build the browser app (apps/lightcraft-web) into <target>/web/;
                  --serve serves it on http://127.0.0.1:<port> (default 8080)
  ci              fmt --check, clippy -D warnings, test, parity refs, layers, assets, wasm (stops at first failure)
  corpus [--download]
                  show where test corpora live; --download fetches PngSuite and CC0 raw samples (raw.pixls.us) into corpus/
  stats [--exact] count tests and lines per crate (--exact: ask the test harness via `-- --list`)
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rest: Vec<&str> = args.iter().skip(1).map(String::as_str).collect();
    let result = match args.first().map(String::as_str) {
        Some("ico") => ico::run(&rest),
        Some("version") => version::run(&root(), &rest),
        Some("layers") => cmd_layers(),
        Some("assets") => assets::run(&root()),
        Some("bench") => bench::run(&root(), &rest),
        Some("parity") => parity::run(&root(), rest.contains(&"--write")),
        Some("wasm") => cmd_wasm(),
        Some("web") => web::run(&rest),
        Some("ci") => cmd_ci(),
        Some("corpus") => cmd_corpus(rest.contains(&"--download")),
        Some("stats") => stats::run(&root(), rest.contains(&"--exact")),
        Some("-h" | "--help" | "help") | None => {
            print!("{USAGE}");
            Ok(())
        }
        Some(other) => Err(format!("unknown command `{other}`\n\n{USAGE}")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Workspace root (parent of the xtask crate).
pub fn root() -> PathBuf {
    // `cargo run` sets it at run time; prefer that over the compile-time value, which goes stale
    // when the checkout moves and the cached xtask binary isn't rebuilt
    let dir = std::env::var_os("CARGO_MANIFEST_DIR").map(PathBuf::from).filter(|d| d.join("Cargo.toml").is_file());
    let dir = dir.unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")));
    dir.parent().expect("xtask has a parent dir").to_path_buf()
}

pub fn cargo() -> Command {
    let mut c = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    c.current_dir(root());
    // Full Windows debuginfo plus one linker per CPU can exhaust RAM before any
    // tests run. Scope these overridable defaults to CI, including parity/WASM
    // subprocesses, without changing ordinary developer builds or GPU coverage.
    if std::env::args().nth(1).as_deref() == Some("ci") {
        static JOBS: std::sync::OnceLock<(String, String)> = std::sync::OnceLock::new();
        let (build, threads) = JOBS.get_or_init(|| {
            let ram_mb = available_ram_mb().or_else(|| total_ram_gb().map(|gb| gb.saturating_mul(1024) / 2));
            let jobs = ci_jobs(ram_mb, std::thread::available_parallelism().map_or(4, |n| n.get()));
            // More test threads barely shorten CI (a few heavy tests dominate) but make the
            // wall-clock frame-budget tests flaky on a loaded machine.
            (jobs.to_string(), jobs.min(4).to_string())
        });
        for (key, value) in
            [("CARGO_PROFILE_DEV_DEBUG", "line-tables-only"), ("CARGO_BUILD_JOBS", build.as_str()), ("RUST_TEST_THREADS", threads.as_str())]
        {
            if std::env::var_os(key).is_none() {
                c.env(key, value);
            }
        }
    }
    c
}

/// CI build jobs: one per 1.5 GB of RAM available when CI starts, at most one per CPU, 4 when it
/// is unknown. With line-table debuginfo, 4 jobs measured about 3 GB of RAM in use and 2 jobs
/// about 1.5 GB, so this leaves about half the available RAM to spare. Counting available rather
/// than installed RAM keeps a busy machine (browsers, VMs, editors) from being overcommitted.
/// Test threads are this, capped at 4.
fn ci_jobs(available_mb: Option<u64>, cpus: usize) -> usize {
    available_mb.map_or(4, |mb| usize::try_from(mb / 1536).unwrap_or(usize::MAX)).clamp(1, cpus.max(1))
}

/// RAM the OS could hand out now (free plus reclaimable cache) in MB, if it reports it.
fn available_ram_mb() -> Option<u64> {
    let output = |program: &str, args: &[&str]| {
        let out = Command::new(program).args(args).output().ok()?;
        Some(String::from_utf8_lossy(&out.stdout).into_owned())
    };
    if cfg!(target_os = "linux") {
        let info = std::fs::read_to_string("/proc/meminfo").ok()?;
        let kb = info.lines().find_map(|l| l.strip_prefix("MemAvailable:"))?.trim().trim_end_matches("kB").trim();
        Some(kb.parse::<u64>().ok()? / 1024)
    } else if cfg!(windows) {
        let cmd = "(Get-CimInstance Win32_PerfFormattedData_PerfOS_Memory).AvailableMBytes";
        output("powershell", &["-NoProfile", "-Command", cmd])?.trim().parse().ok()
    } else {
        // macOS: free + inactive + speculative pages
        let stat = output("vm_stat", &[])?;
        let page: u64 = stat.split("page size of ").nth(1)?.split_whitespace().next()?.parse().ok()?;
        let pages = |name: &str| -> Option<u64> {
            let line = stat.lines().find(|l| l.starts_with(name))?;
            line.rsplit(':').next()?.trim().trim_end_matches('.').parse().ok()
        };
        let free = pages("Pages free")?.saturating_add(pages("Pages inactive")?).saturating_add(pages("Pages speculative").unwrap_or(0));
        Some(free.saturating_mul(page) >> 20)
    }
}

/// Installed physical memory in whole GB, if the OS reports it (the fallback when available RAM
/// can't be read: half of it is assumed available).
fn total_ram_gb() -> Option<u64> {
    let bytes: u64 = if cfg!(target_os = "linux") {
        let info = std::fs::read_to_string("/proc/meminfo").ok()?;
        let kb = info.lines().find_map(|l| l.strip_prefix("MemTotal:"))?.trim().trim_end_matches("kB").trim();
        kb.parse::<u64>().ok()?.checked_mul(1024)?
    } else {
        let (program, args): (&str, &[&str]) = if cfg!(windows) {
            ("powershell", &["-NoProfile", "-Command", "(Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory"])
        } else {
            ("sysctl", &["-n", "hw.memsize"])
        };
        let out = Command::new(program).args(args).output().ok()?;
        String::from_utf8_lossy(&out.stdout).trim().parse().ok()?
    };
    // round to the nearest GB: "8 GB" machines report slightly less
    Some(bytes.saturating_add(1 << 29) >> 30)
}

pub fn run(mut cmd: Command, what: &str) -> Result<(), String> {
    eprintln!("$ {what}");
    let status = cmd.status().map_err(|e| format!("{what}: failed to spawn: {e}"))?;
    if status.success() { Ok(()) } else { Err(format!("{what}: exited with {status}")) }
}

pub fn metadata() -> Result<serde_json::Value, String> {
    let out = cargo().args(["metadata", "--format-version", "1", "--no-deps"]).output().map_err(|e| format!("cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err(format!("cargo metadata failed:\n{}", String::from_utf8_lossy(&out.stderr)));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| format!("cargo metadata: bad JSON: {e}"))
}

fn cmd_layers() -> Result<(), String> {
    let crates = layers::from_metadata(&metadata()?)?;
    println!("Dependency layering (plan/architecture.md §3)\n");
    println!("{:<28} {:<14} workspace deps", "crate", "layer");
    for c in &crates {
        let ws: Vec<String> = c
            .deps
            .iter()
            .filter(|d| d.workspace)
            .map(|d| {
                let k = match d.kind {
                    layers::DepKind::Normal => "",
                    layers::DepKind::Dev => " (dev)",
                    layers::DepKind::Build => " (build)",
                };
                format!("{}{k}", layers::short_name(&d.name))
            })
            .collect();
        println!("{:<28} {:<14} {}", c.name, layers::describe(layers::classify(&c.name)), ws.join(", "));
    }
    let violations = layers::check(&crates);
    println!();
    if violations.is_empty() {
        println!("OK: {} crates, no layering violations.", crates.len());
        Ok(())
    } else {
        println!("{} violation(s):", violations.len());
        for v in &violations {
            println!("  - {v}");
        }
        Err(format!("{} layering violation(s)", violations.len()))
    }
}

/// Workspace packages that must build for wasm32: all L0–L5 crates plus
/// the egui shell and the web app.
fn wasm_set() -> Result<Vec<String>, String> {
    let crates = layers::from_metadata(&metadata()?)?;
    Ok(crates
        .into_iter()
        .filter(|c| match layers::classify(&c.name) {
            Some(layers::Class::Layer(l)) => l <= 5,
            Some(layers::Class::Standalone) => true,
            _ => false,
        })
        .map(|c| c.name)
        .chain(std::iter::once("lightcraft-web".to_string()))
        .collect())
}

fn cmd_wasm() -> Result<(), String> {
    let set = wasm_set()?;
    let mut results = Vec::new();
    for pkg in &set {
        let mut c = cargo();
        c.args(["check", "--target", "wasm32-unknown-unknown", "-p", pkg]);
        let ok = run(c, &format!("cargo check --target wasm32-unknown-unknown -p {pkg}")).is_ok();
        results.push((pkg.clone(), ok));
    }
    println!("\nwasm32-unknown-unknown check:");
    for (p, ok) in &results {
        println!("  {:<6} {p}", if *ok { "ok" } else { "FAIL" });
    }
    let failed = results.iter().filter(|r| !r.1).count();
    if failed == 0 { Ok(()) } else { Err(format!("{failed} crate(s) failed the wasm check")) }
}

fn cmd_ci() -> Result<(), String> {
    type Step = (&'static str, Box<dyn Fn() -> Result<(), String>>);
    let steps: Vec<Step> = vec![
        (
            "fmt",
            Box::new(|| {
                let mut c = cargo();
                c.args(["fmt", "--all", "--", "--check"]);
                run(c, "cargo fmt --all -- --check")
            }),
        ),
        (
            "clippy",
            Box::new(|| {
                let mut c = cargo();
                c.args(["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"]);
                run(c, "cargo clippy --workspace --all-targets -- -D warnings")
            }),
        ),
        (
            "test",
            Box::new(|| {
                let mut c = cargo();
                c.args(["test", "--workspace"]);
                run(c, "cargo test --workspace")
            }),
        ),
        ("parity", Box::new(|| parity::run(&root(), false))),
        ("layers", Box::new(cmd_layers)),
        ("assets", Box::new(|| assets::run(&root()))),
        ("wasm", Box::new(cmd_wasm)),
    ];
    let mut done = Vec::new();
    for (name, f) in &steps {
        eprintln!("\n=== ci: {name} ===");
        if let Err(e) = f() {
            println!("\nCI summary:");
            for d in &done {
                println!("  ok    {d}");
            }
            println!("  FAIL  {name}: {e}");
            for (n, _) in steps.iter().skip(done.len() + 1) {
                println!("  skip  {n}");
            }
            return Err(format!("ci failed at `{name}`"));
        }
        done.push(*name);
    }
    println!("\nCI summary: all {} steps passed ({})", done.len(), done.join(", "));
    Ok(())
}

/// CC0 raw samples (raw.pixls.us). Small, representative set; extend freely (CC0 only).
/// CC0 raw samples from raw.pixls.us (each verified CC0 on the site; files matched by their published SHA-256).
/// One per format / compression variant we decode or deliberately report as unsupported (preview only).
const RAW_SAMPLES: &[(&str, &str)] = &[
    (
        "arw-sony-a7m3-compressed.arw",
        "https://raw.pixls.us/getfile.php/2414/nice/Sony%20-%20ILCE-7M3%20-%2014bit%2014bit%20compressed%20%283:2%29.ARW",
    ),
    (
        "arw-sony-a7m3-uncompressed.arw",
        "https://raw.pixls.us/getfile.php/2418/nice/Sony%20-%20ILCE-7M3%20-%2014bit%2014bit%20uncompressed%20%283:2%29.ARW",
    ),
    ("arw-sony-a7m4-14bit.arw", "https://raw.pixls.us/getfile.php/6936/nice/Sony%20-%20ILCE-7M4%20-%2014bit%20%283:2%29.ARW"),
    // lossless compressed (Compression 7): L is 2×2 CFA cells per LJ92 sample; M and S are subsampled
    ("arw-sony-a7m4-lossless-l.arw", "https://raw.pixls.us/data/Sony/ILCE-7M4/ILCE-7M4_DSC06674_FullFrame-LossLess-Compressed-Large.ARW"),
    ("arw-sony-a7m4-lossless-m.arw", "https://raw.pixls.us/data/Sony/ILCE-7M4/ILCE-7M4_DSC06675_FullFrame-LossLess-Compressed-Medium.ARW"),
    ("arw-sony-a7m4-lossless-s.arw", "https://raw.pixls.us/data/Sony/ILCE-7M4/ILCE-7M4_DSC06676_FullFrame-LossLess-Compressed-Small.ARW"),
    // pre-2017 bodies: white balance only in the enciphered maker note, black level only in the SR2SubIFD (#148)
    ("arw-sony-rx100m3.arw", "https://raw.pixls.us/data/Sony/DSC-RX100M3/DSC00734.ARW"),
    ("arw-sony-rx100.arw", "https://raw.pixls.us/data/Sony/DSC-RX100/DSC00838.ARW"),
    ("arw-sony-a7rm2-12bit-uncompressed.arw", "https://raw.pixls.us/data/Sony/ILCE-7RM2/12-bit-uncompressed.ARW"),
    // CR2 colour-filter layouts differ by model (issue #85): CR2CFAPattern 3 (GBRG) and 1 (RGGB) samples
    ("cr2-canon-40d.cr2", "https://raw.pixls.us/data/Canon/EOS%2040D/_MG_0153.CR2"),
    ("cr2-canon-550d.cr2", "https://raw.pixls.us/data/Canon/EOS%20550D/IMG_4047.CR2"),
    ("cr2-canon-5d2.cr2", "https://raw.pixls.us/data/Canon/EOS%205D%20Mark%20II/08.canon.raw.cr2"),
    ("cr2-canon-5dsr.cr2", "https://raw.pixls.us/data/Canon/EOS%205DS%20R/_DSR2002.CR2"),
    ("cr2-canon-6d.cr2", "https://raw.pixls.us/data/Canon/EOS%206D/EOS_6D_RAW.CR2"),
    ("cr2-canon-7d.cr2", "https://raw.pixls.us/data/Canon/EOS%207D/RAW_CANON_EOS_7D-raw.CR2"),
    ("cr2-canon-5d3-sraw2.cr2", "https://raw.pixls.us/getfile.php/773/nice/Canon%20-%20EOS%205D%20Mark%20III%20-%20sRAW2%20%28sRAW%29.CR2"),
    ("cr2-canon-5d3.cr2", "https://raw.pixls.us/getfile.php/771/nice/Canon%20-%20EOS%205D%20Mark%20III.CR2"),
    ("cr2-canon-80d.cr2", "https://raw.pixls.us/getfile.php/1294/nice/Canon%20-%20EOS%2080D%20-%20RAW%20%283:2%29.CR2"),
    ("cr3-canon-m50-craw.cr3", "https://raw.pixls.us/getfile.php/2663/nice/Canon%20-%20EOS%20M50%20-%20CRAW%20%283:2%29.CR3"),
    ("cr3-canon-m50-raw.cr3", "https://raw.pixls.us/getfile.php/4657/nice/Canon%20-%20EOS%20M50%20-%203:2.CR3"),
    ("cr3-canon-r100-raw.cr3", "https://raw.pixls.us/getfile.php/7896/nice/Canon%20-%20EOS%20R100%20-%20RAW%20%283:2%29.CR3"),
    ("cr3-canon-r100-craw.cr3", "https://raw.pixls.us/getfile.php/7897/nice/Canon%20-%20EOS%20R100%20-%20CRAW%20%283:2%29.CR3"),
    ("cr3-canon-r8-raw.cr3", "https://raw.pixls.us/getfile.php/6585/nice/Canon%20-%20EOS%20R8%20-%203:2.CR3"),
    ("cr3-canon-r8-craw.cr3", "https://raw.pixls.us/getfile.php/6587/nice/Canon%20-%20EOS%20R8%20-%203:2.CR3"),
    // The complete raw.pixls.us Canon CR3 tree (2026-10-09 sweep, 110 files over 29 bodies; CC0).
    // The six `getfile.php` entries above are byte-identical to six data-tree files (SHA-256 verified);
    // everything below adds the remaining files, each pinned in `CR3_SHA256` beneath this table.
    ("cr3-canon-eos-m200-mg-2231.cr3", "https://raw.pixls.us/data/Canon/Canon%20EOS%20M200/_MG_2231.CR3"),
    ("cr3-canon-eos-m200-mg-2233.cr3", "https://raw.pixls.us/data/Canon/Canon%20EOS%20M200/_MG_2233.CR3"),
    ("cr3-canon-eos-r10-img-6310.cr3", "https://raw.pixls.us/data/Canon/Canon%20EOS%20R10/IMG_6310.CR3"),
    ("cr3-canon-eos-r50-v-img-0034.cr3", "https://raw.pixls.us/data/Canon/Canon%20EOS%20R50%20V/IMG_0034.CR3"),
    ("cr3-canon-eos-r5m2-aps-c-craw.cr3", "https://raw.pixls.us/data/Canon/Canon%20EOS%20R5m2/APS-C_CRAW.CR3"),
    ("cr3-canon-eos-r5m2-aps-c-raw.cr3", "https://raw.pixls.us/data/Canon/Canon%20EOS%20R5m2/APS-C_RAW.CR3"),
    ("cr3-canon-eos-r5m2-craw.cr3", "https://raw.pixls.us/data/Canon/Canon%20EOS%20R5m2/CRAW.CR3"),
    ("cr3-canon-eos-r5m2-raw.cr3", "https://raw.pixls.us/data/Canon/Canon%20EOS%20R5m2/RAW.CR3"),
    ("cr3-canon-eos-r6-mark-iii-img-4241.cr3", "https://raw.pixls.us/data/Canon/Canon%20EOS%20R6%20Mark%20III/IMG_4241.CR3"),
    ("cr3-canon-eos-r6-mark-iii-img-4242.cr3", "https://raw.pixls.us/data/Canon/Canon%20EOS%20R6%20Mark%20III/IMG_4242.CR3"),
    ("cr3-canon-eos-r6-mark-iii-img-4243.cr3", "https://raw.pixls.us/data/Canon/Canon%20EOS%20R6%20Mark%20III/IMG_4243.CR3"),
    ("cr3-canon-eos-r6-mark-iii-img-4244.cr3", "https://raw.pixls.us/data/Canon/Canon%20EOS%20R6%20Mark%20III/IMG_4244.CR3"),
    ("cr3-canon-powershot-v1-img-1225.cr3", "https://raw.pixls.us/data/Canon/Canon%20PowerShot%20V1/IMG_1225.CR3"),
    (
        "cr3-eos-1d-x-mark-iii-canon-eos-1d-x-mark-iii-craw-iso-100-nocrop-nodual.cr3",
        "https://raw.pixls.us/data/Canon/EOS-1D%20X%20Mark%20III/Canon_EOS-1D_X_Mark_III_CRAW_ISO_100_nocrop_nodual.CR3",
    ),
    (
        "cr3-eos-1d-x-mark-iii-canon-eos-1d-x-mark-iii-raw-iso-100-nocrop-nodual.cr3",
        "https://raw.pixls.us/data/Canon/EOS-1D%20X%20Mark%20III/Canon_EOS-1D_X_Mark_III_RAW_ISO_100_nocrop_nodual.CR3",
    ),
    ("cr3-eos-200d-mark-ii-mg-3608.cr3", "https://raw.pixls.us/data/Canon/EOS%20200D%20Mark%20II/_MG_3608.CR3"),
    ("cr3-eos-250d-canon250d-craw-1-1.cr3", "https://raw.pixls.us/data/Canon/EOS%20250D/./Canon250D_cRAW-1:1.CR3"),
    ("cr3-eos-250d-canon250d-craw-16-9.cr3", "https://raw.pixls.us/data/Canon/EOS%20250D/./Canon250D_cRAW-16:9.CR3"),
    ("cr3-eos-250d-canon250d-craw-3-2.cr3", "https://raw.pixls.us/data/Canon/EOS%20250D/./Canon250D_cRAW-3:2.CR3"),
    ("cr3-eos-250d-canon250d-craw-4-3.cr3", "https://raw.pixls.us/data/Canon/EOS%20250D/./Canon250D_cRAW-4:3.CR3"),
    ("cr3-eos-250d-canon250d-raw-1-1.cr3", "https://raw.pixls.us/data/Canon/EOS%20250D/./Canon250D_RAW-1:1.CR3"),
    ("cr3-eos-250d-canon250d-raw-19-9.cr3", "https://raw.pixls.us/data/Canon/EOS%20250D/./Canon250D_RAW-19:9.CR3"),
    ("cr3-eos-250d-canon250d-raw-2-3.cr3", "https://raw.pixls.us/data/Canon/EOS%20250D/./Canon250D_RAW-2:3.CR3"),
    ("cr3-eos-250d-canon250d-raw-4-3.cr3", "https://raw.pixls.us/data/Canon/EOS%20250D/./Canon250D_RAW-4:3.CR3"),
    ("cr3-eos-850d-img-1837.cr3", "https://raw.pixls.us/data/Canon/EOS%20850D/IMG_1837.CR3"),
    ("cr3-eos-850d-img-1906.cr3", "https://raw.pixls.us/data/Canon/EOS%20850D/IMG_1906.CR3"),
    ("cr3-eos-90d-canon-90d-craw-iso-100.cr3", "https://raw.pixls.us/data/Canon/EOS%2090D/Canon-90D-cRAW-ISO-100.CR3"),
    ("cr3-eos-90d-canon-90d-raw-iso-100.cr3", "https://raw.pixls.us/data/Canon/EOS%2090D/Canon-90D-RAW-ISO-100.CR3"),
    ("cr3-eos-kiss-m-img-2695.cr3", "https://raw.pixls.us/data/Canon/EOS%20Kiss%20M/IMG_2695.CR3"),
    (
        "cr3-eos-m50-canon-eos-m50-craw-iso-100-nocrop-nodual.cr3",
        "https://raw.pixls.us/data/Canon/EOS%20M50/Canon_EOS_M50_CRAW_ISO_100_nocrop_nodual.CR3",
    ),
    ("cr3-eos-m50-mark-ii-img-0051.cr3", "https://raw.pixls.us/data/Canon/EOS%20M50%20Mark%20II/IMG_0051.CR3"),
    ("cr3-eos-m6-mark-ii-img-craw-1-1.cr3", "https://raw.pixls.us/data/Canon/EOS%20M6%20Mark%20II/IMG_CRAW_1_1.CR3"),
    ("cr3-eos-m6-mark-ii-img-craw-16-9.cr3", "https://raw.pixls.us/data/Canon/EOS%20M6%20Mark%20II/IMG_CRAW_16_9.CR3"),
    ("cr3-eos-m6-mark-ii-img-craw-3-2.cr3", "https://raw.pixls.us/data/Canon/EOS%20M6%20Mark%20II/IMG_CRAW_3_2.CR3"),
    ("cr3-eos-m6-mark-ii-img-craw-4-3.cr3", "https://raw.pixls.us/data/Canon/EOS%20M6%20Mark%20II/IMG_CRAW_4_3.CR3"),
    ("cr3-eos-m6-mark-ii-img-raw-1-1.cr3", "https://raw.pixls.us/data/Canon/EOS%20M6%20Mark%20II/IMG_RAW_1_1.CR3"),
    ("cr3-eos-m6-mark-ii-img-raw-16-9.cr3", "https://raw.pixls.us/data/Canon/EOS%20M6%20Mark%20II/IMG_RAW_16_9.CR3"),
    ("cr3-eos-m6-mark-ii-img-raw-3-2.cr3", "https://raw.pixls.us/data/Canon/EOS%20M6%20Mark%20II/IMG_RAW_3_2.CR3"),
    ("cr3-eos-m6-mark-ii-img-raw-4-3.cr3", "https://raw.pixls.us/data/Canon/EOS%20M6%20Mark%20II/IMG_RAW_4_3.CR3"),
    ("cr3-eos-r-canon-eos-r-craw-iso-100-crop-dual.cr3", "https://raw.pixls.us/data/Canon/EOS%20R/Canon_EOS_R_CRAW_ISO_100_crop_dual.CR3"),
    ("cr3-eos-r-canon-eos-r-craw-iso-100-crop-nodual.cr3", "https://raw.pixls.us/data/Canon/EOS%20R/Canon_EOS_R_CRAW_ISO_100_crop_nodual.CR3"),
    ("cr3-eos-r-canon-eos-r-craw-iso-100-dual-pixel-on.cr3", "https://raw.pixls.us/data/Canon/EOS%20R/Canon_EOS_R_CRAW_ISO_100_dual_pixel_on.CR3"),
    ("cr3-eos-r-canon-eos-r-craw-iso-100.cr3", "https://raw.pixls.us/data/Canon/EOS%20R/Canon_EOS_R_CRAW_ISO_100.CR3"),
    ("cr3-eos-r-canon-eos-r-raw-iso-100-crop-dual.cr3", "https://raw.pixls.us/data/Canon/EOS%20R/Canon_EOS_R_RAW_ISO_100_crop_dual.CR3"),
    ("cr3-eos-r-canon-eos-r-raw-iso-100-crop-nodual.cr3", "https://raw.pixls.us/data/Canon/EOS%20R/Canon_EOS_R_RAW_ISO_100_crop_nodual.CR3"),
    ("cr3-eos-r-canon-eos-r-raw-iso-100-dual-pixel-on.cr3", "https://raw.pixls.us/data/Canon/EOS%20R/Canon_EOS_R_RAW_ISO_100_dual_pixel_on.CR3"),
    ("cr3-eos-r-canon-eos-r-raw-iso-100.cr3", "https://raw.pixls.us/data/Canon/EOS%20R/Canon_EOS_R_RAW_ISO_100.CR3"),
    ("cr3-eos-r10-img-4276.cr3", "https://raw.pixls.us/data/Canon/EOS%20R10/IMG_4276.CR3"),
    ("cr3-eos-r10-img-4279.cr3", "https://raw.pixls.us/data/Canon/EOS%20R10/IMG_4279.CR3"),
    ("cr3-eos-r10-img-4361.cr3", "https://raw.pixls.us/data/Canon/EOS%20R10/IMG_4361.CR3"),
    ("cr3-eos-r3-c3a1200.cr3", "https://raw.pixls.us/data/Canon/EOS%20R3/_C3A1200.CR3"),
    ("cr3-eos-r5-canon-eos-r5-craw-iso-100-crop-dual.cr3", "https://raw.pixls.us/data/Canon/EOS%20R5/Canon_EOS_R5_CRAW_ISO_100_crop_dual.CR3"),
    ("cr3-eos-r5-canon-eos-r5-craw-iso-100-crop-nodual.cr3", "https://raw.pixls.us/data/Canon/EOS%20R5/Canon_EOS_R5_CRAW_ISO_100_crop_nodual.CR3"),
    ("cr3-eos-r5-canon-eos-r5-craw-iso-100-nocrop-dual.cr3", "https://raw.pixls.us/data/Canon/EOS%20R5/Canon_EOS_R5_CRAW_ISO_100_nocrop_dual.CR3"),
    (
        "cr3-eos-r5-canon-eos-r5-craw-iso-100-nocrop-nodual.cr3",
        "https://raw.pixls.us/data/Canon/EOS%20R5/Canon_EOS_R5_CRAW_ISO_100_nocrop_nodual.CR3",
    ),
    ("cr3-eos-r5-canon-eos-r5-raw-iso-100-crop-dual.cr3", "https://raw.pixls.us/data/Canon/EOS%20R5/Canon_EOS_R5_RAW_ISO_100_crop_dual.CR3"),
    ("cr3-eos-r5-canon-eos-r5-raw-iso-100-crop-nodual.cr3", "https://raw.pixls.us/data/Canon/EOS%20R5/Canon_EOS_R5_RAW_ISO_100_crop_nodual.CR3"),
    ("cr3-eos-r5-canon-eos-r5-raw-iso-100-nocrop-dual.cr3", "https://raw.pixls.us/data/Canon/EOS%20R5/Canon_EOS_R5_RAW_ISO_100_nocrop_dual.CR3"),
    ("cr3-eos-r5-canon-eos-r5-raw-iso-100-nocrop-nodual.cr3", "https://raw.pixls.us/data/Canon/EOS%20R5/Canon_EOS_R5_RAW_ISO_100_nocrop_nodual.CR3"),
    (
        "cr3-eos-r50-canon-r50-iso100-craw-electronicshutter.cr3",
        "https://raw.pixls.us/data/Canon/EOS%20R50/Canon_R50_ISO100_cRAW_electronicShutter.CR3",
    ),
    ("cr3-eos-r50-canon-r50-iso100-craw-mechanicshutter.cr3", "https://raw.pixls.us/data/Canon/EOS%20R50/Canon_R50_ISO100_cRAW_mechanicShutter.CR3"),
    (
        "cr3-eos-r50-canon-r50-iso100-raw-electronicshutter.cr3",
        "https://raw.pixls.us/data/Canon/EOS%20R50/Canon_R50_ISO100_RAW_electronicShutter.CR3",
    ),
    ("cr3-eos-r50-canon-r50-iso100-raw-mechanicshutter.cr3", "https://raw.pixls.us/data/Canon/EOS%20R50/Canon_R50_ISO100_RAW_mechanicShutter.CR3"),
    ("cr3-eos-r6-canon-eos-r6-craw-iso-100-crop-nodual.cr3", "https://raw.pixls.us/data/Canon/EOS%20R6/Canon_EOS_R6_CRAW_ISO_100_crop_nodual.CR3"),
    (
        "cr3-eos-r6-canon-eos-r6-craw-iso-100-nocrop-nodual.cr3",
        "https://raw.pixls.us/data/Canon/EOS%20R6/Canon_EOS_R6_CRAW_ISO_100_nocrop_nodual.CR3",
    ),
    ("cr3-eos-r6-canon-eos-r6-raw-iso-100-crop-nodual.cr3", "https://raw.pixls.us/data/Canon/EOS%20R6/Canon_EOS_R6_RAW_ISO_100_crop_nodual.CR3"),
    ("cr3-eos-r6-canon-eos-r6-raw-iso-100-nocrop-nodual.cr3", "https://raw.pixls.us/data/Canon/EOS%20R6/Canon_EOS_R6_RAW_ISO_100_nocrop_nodual.CR3"),
    (
        "cr3-eos-r6-mark-ii-canon-r6m2-img-0346-raw-normal-full.cr3",
        "https://raw.pixls.us/data/Canon/EOS%20R6%20Mark%20II/canon-r6m2-img_0346-raw-normal-full.cr3",
    ),
    (
        "cr3-eos-r6-mark-ii-canon-r6m2-img-0347-raw-normal-crop.cr3",
        "https://raw.pixls.us/data/Canon/EOS%20R6%20Mark%20II/canon-r6m2-img_0347-raw-normal-crop.cr3",
    ),
    (
        "cr3-eos-r6-mark-ii-canon-r6m2-img-0349-raw-dual-crop.cr3",
        "https://raw.pixls.us/data/Canon/EOS%20R6%20Mark%20II/canon-r6m2-img_0349-raw-dual-crop.cr3",
    ),
    (
        "cr3-eos-r6-mark-ii-canon-r6m2-img-0351-raw-dual-full.cr3",
        "https://raw.pixls.us/data/Canon/EOS%20R6%20Mark%20II/canon-r6m2-img_0351-raw-dual-full.cr3",
    ),
    (
        "cr3-eos-r6-mark-ii-canon-r6m2-img-0355-craw-dual-full.cr3",
        "https://raw.pixls.us/data/Canon/EOS%20R6%20Mark%20II/canon-r6m2-img_0355-craw-dual-full.cr3",
    ),
    (
        "cr3-eos-r6-mark-ii-canon-r6m2-img-0356-craw-dual-crop.cr3",
        "https://raw.pixls.us/data/Canon/EOS%20R6%20Mark%20II/canon-r6m2-img_0356-craw-dual-crop.cr3",
    ),
    (
        "cr3-eos-r6-mark-ii-canon-r6m2-img-0359-craw-normal-crop.cr3",
        "https://raw.pixls.us/data/Canon/EOS%20R6%20Mark%20II/canon-r6m2-img_0359-craw-normal-crop.cr3",
    ),
    (
        "cr3-eos-r6-mark-ii-canon-r6m2-img-0361-craw-normal-full.cr3",
        "https://raw.pixls.us/data/Canon/EOS%20R6%20Mark%20II/canon-r6m2-img_0361-craw-normal-full.cr3",
    ),
    ("cr3-eos-r7-443a0157.cr3", "https://raw.pixls.us/data/Canon/EOS%20R7/443A0157.CR3"),
    ("cr3-eos-r7-443a0159.cr3", "https://raw.pixls.us/data/Canon/EOS%20R7/443A0159.CR3"),
    ("cr3-eos-r7-443a0161.cr3", "https://raw.pixls.us/data/Canon/EOS%20R7/443A0161.CR3"),
    ("cr3-eos-r7-443a0163.cr3", "https://raw.pixls.us/data/Canon/EOS%20R7/443A0163.CR3"),
    ("cr3-eos-r8-craw-full-frame-dual-pixel.cr3", "https://raw.pixls.us/data/Canon/EOS%20R8/CRAW_FULL_FRAME_DUAL_PIXEL.CR3"),
    ("cr3-eos-r8-raw-full-frame-dual-pixel.cr3", "https://raw.pixls.us/data/Canon/EOS%20R8/RAW_FULL_FRAME_DUAL_PIXEL.CR3"),
    ("cr3-eos-rebel-sl3-sedona2020-sl3-mg-1061.cr3", "https://raw.pixls.us/data/Canon/EOS%20Rebel%20SL3/Sedona2020_SL3__MG_1061.CR3"),
    ("cr3-eos-rp-canon-eos-rp-craw-iso-100-crop-nodual.cr3", "https://raw.pixls.us/data/Canon/EOS%20RP/Canon_EOS_RP_CRAW_ISO_100_crop_nodual.CR3"),
    (
        "cr3-eos-rp-canon-eos-rp-craw-iso-100-nocrop-nodual.cr3",
        "https://raw.pixls.us/data/Canon/EOS%20RP/Canon_EOS_RP_CRAW_ISO_100_nocrop_nodual.CR3",
    ),
    ("cr3-eos-rp-canon-eos-rp-raw-iso-100-crop-nodual.cr3", "https://raw.pixls.us/data/Canon/EOS%20RP/Canon_EOS_RP_RAW_ISO_100_crop_nodual.CR3"),
    ("cr3-eos-rp-canon-eos-rp-raw-iso-100-nocrop-nodual.cr3", "https://raw.pixls.us/data/Canon/EOS%20RP/Canon_EOS_RP_RAW_ISO_100_nocrop_nodual.CR3"),
    (
        "cr3-powershot-g5-x-mark-ii-canon-powershot-g5-x-mark-ii-craw-1-1.cr3",
        "https://raw.pixls.us/data/Canon/PowerShot%20G5%20X%20Mark%20II/./Canon_-_PowerShot_G5_X_Mark_II_-_CRAW_(1:1).CR3",
    ),
    (
        "cr3-powershot-g5-x-mark-ii-canon-powershot-g5-x-mark-ii-craw-16-9.cr3",
        "https://raw.pixls.us/data/Canon/PowerShot%20G5%20X%20Mark%20II/./Canon_-_PowerShot_G5_X_Mark_II_-_CRAW_(16:9).CR3",
    ),
    (
        "cr3-powershot-g5-x-mark-ii-canon-powershot-g5-x-mark-ii-craw-3-2.cr3",
        "https://raw.pixls.us/data/Canon/PowerShot%20G5%20X%20Mark%20II/./Canon_-_PowerShot_G5_X_Mark_II_-_CRAW_(3:2).CR3",
    ),
    (
        "cr3-powershot-g5-x-mark-ii-canon-powershot-g5-x-mark-ii-craw-4-3.cr3",
        "https://raw.pixls.us/data/Canon/PowerShot%20G5%20X%20Mark%20II/./Canon_-_PowerShot_G5_X_Mark_II_-_CRAW_(4:3).CR3",
    ),
    (
        "cr3-powershot-g5-x-mark-ii-canon-powershot-g5-x-mark-ii-raw-1-1.cr3",
        "https://raw.pixls.us/data/Canon/PowerShot%20G5%20X%20Mark%20II/./Canon_-_PowerShot_G5_X_Mark_II_-_RAW_(1:1).CR3",
    ),
    (
        "cr3-powershot-g5-x-mark-ii-canon-powershot-g5-x-mark-ii-raw-16-9.cr3",
        "https://raw.pixls.us/data/Canon/PowerShot%20G5%20X%20Mark%20II/./Canon_-_PowerShot_G5_X_Mark_II_-_RAW_(16:9).CR3",
    ),
    (
        "cr3-powershot-g5-x-mark-ii-canon-powershot-g5-x-mark-ii-raw-3-2.cr3",
        "https://raw.pixls.us/data/Canon/PowerShot%20G5%20X%20Mark%20II/./Canon_-_PowerShot_G5_X_Mark_II_-_RAW_(3:2).CR3",
    ),
    (
        "cr3-powershot-g5-x-mark-ii-canon-powershot-g5-x-mark-ii-raw-4-3.cr3",
        "https://raw.pixls.us/data/Canon/PowerShot%20G5%20X%20Mark%20II/./Canon_-_PowerShot_G5_X_Mark_II_-_RAW_(4:3).CR3",
    ),
    ("cr3-powershot-g7-x-mark-iii-img-0253.cr3", "https://raw.pixls.us/data/Canon/PowerShot%20G7%20X%20Mark%20III/IMG_0253.CR3"),
    ("cr3-powershot-g7-x-mark-iii-img-0254.cr3", "https://raw.pixls.us/data/Canon/PowerShot%20G7%20X%20Mark%20III/IMG_0254.CR3"),
    ("cr3-powershot-sx70-hs-img-1000.cr3", "https://raw.pixls.us/data/Canon/PowerShot%20SX70%20HS/IMG_1000.CR3"),
    ("cr3-powershot-sx70-hs-img-1001.cr3", "https://raw.pixls.us/data/Canon/PowerShot%20SX70%20HS/IMG_1001.CR3"),
    ("cr3-powershot-sx70-hs-img-1002.cr3", "https://raw.pixls.us/data/Canon/PowerShot%20SX70%20HS/IMG_1002.CR3"),
    ("cr3-powershot-sx70-hs-img-1003.cr3", "https://raw.pixls.us/data/Canon/PowerShot%20SX70%20HS/IMG_1003.CR3"),
    ("cr3-powershot-sx70-hs-img-1004.cr3", "https://raw.pixls.us/data/Canon/PowerShot%20SX70%20HS/IMG_1004.CR3"),
    ("cr3-powershot-sx70-hs-img-1005.cr3", "https://raw.pixls.us/data/Canon/PowerShot%20SX70%20HS/IMG_1005.CR3"),
    ("cr3-powershot-sx70-hs-img-1006.cr3", "https://raw.pixls.us/data/Canon/PowerShot%20SX70%20HS/IMG_1006.CR3"),
    ("cr3-powershot-sx70-hs-img-1007.cr3", "https://raw.pixls.us/data/Canon/PowerShot%20SX70%20HS/IMG_1007.CR3"),
    (
        "dng-adobe-canon-5d3-linear-lj92.dng",
        "https://raw.pixls.us/getfile.php/1032/nice/Adobe%20DNG%20Converter%20-%20Canon%20EOS%205D%20Mark%20III%20-%20Lossless%20JPEG%20compression%2C%20rgb%20%283:2%29.DNG",
    ),
    (
        "dng-adobe-canon-5d3-lj92.dng",
        "https://raw.pixls.us/getfile.php/1024/nice/Adobe%20DNG%20Converter%20-%20Canon%20EOS%205D%20Mark%20III%20-%2016bit%2016bit%20Lossless%20JPEG%20compression%20%283:2%29.DNG",
    ),
    (
        "dng-adobe-canon-5d3-lossy.dng",
        "https://raw.pixls.us/getfile.php/1023/nice/Adobe%20DNG%20Converter%20-%20Canon%20EOS%205D%20Mark%20III%20-%20Lossy%20JPEG%20compression%20%283:2%29.DNG",
    ),
    (
        "dng-canon-5d3-14bit-small.dng",
        "https://raw.pixls.us/getfile.php/2204/nice/Canon%20-%20EOS%205D%20Mark%20III%20-%2014bit%2014bit%20%282.3471882640587%29.dng",
    ),
    ("dng-canon-5d3-16bit-169.dng", "https://raw.pixls.us/getfile.php/2649/nice/Canon%20-%20EOS%205D%20Mark%20III%20-%2016bit%20%2816:9%29.dng"),
    ("dng-canon-5d3-16bit.dng", "https://raw.pixls.us/getfile.php/885/nice/Canon%20-%20EOS%205D%20Mark%20III%20-%2016bit%2016bit%20RAW.dng"),
    ("dng-google-pixel2xl.dng", "https://raw.pixls.us/getfile.php/2206/nice/Google%20-%20Pixel%202%20XL%20-%2016bit%20%284:3%29.dng"),
    ("dng-ricoh-gr3.dng", "https://raw.pixls.us/getfile.php/3115/nice/Ricoh%20-%20GR%20III%20-%2014bit%20%283:2%29.DNG"),
    (
        "nef-nikon-d5100-lossless.nef",
        "https://raw.pixls.us/getfile.php/1597/nice/Nikon%20-%20D5100%20-%2014bit%2014bit%20compressed%20%28Lossless%29%20%283:2%29.nef",
    ),
    (
        "nef-nikon-d5100-uncompressed.nef",
        "https://raw.pixls.us/getfile.php/1598/nice/Nikon%20-%20D5100%20-%2014bit%2014bit%20uncompressed%20%283:2%29.nef",
    ),
    (
        "nef-nikon-d7000-lossy12.nef",
        "https://raw.pixls.us/getfile.php/961/nice/Nikon%20-%20D7000%20-%2012bit%2012bit%20compressed%20%28Lossy%20%28type%202%29%29%20%283:2%29.NEF",
    ),
    (
        "nef-nikon-d7500-lossless12.nef",
        "https://raw.pixls.us/getfile.php/1532/nice/Nikon%20-%20D7500%20-%2012bit%2012bit%20compressed%20%28Lossless%29%20%283:2%29.NEF",
    ),
    (
        "nef-nikon-d7500-lossless14.nef",
        "https://raw.pixls.us/getfile.php/1534/nice/Nikon%20-%20D7500%20-%2014bit%2014bit%20compressed%20%28Lossless%29%20%283:2%29.NEF",
    ),
    (
        "nrw-nikon-b700-uncompressed.nrw",
        "https://raw.pixls.us/getfile.php/1621/nice/Nikon%20-%20COOLPIX%20B700%20-%2012bit%2012bit%20uncompressed%20%284:3%29.NRW",
    ),
    ("orf-olympus-e1.orf", "https://raw.pixls.us/getfile.php/1800/nice/Olympus%20-%20E-1%20-%2016bit%20%284:3%29.ORF"),
    ("orf-olympus-e400.orf", "https://raw.pixls.us/getfile.php/2151/nice/Olympus%20-%20E-400%20-%2016bit%20%284:3%29.ORF"),
    ("orf-olympus-em1.orf", "https://raw.pixls.us/getfile.php/1051/nice/Olympus%20-%20E-M1%20-%2016bit%20%284:3%29.orf"),
    ("orf-olympus-em10iii.orf", "https://raw.pixls.us/getfile.php/1787/nice/Olympus%20-%20E-M10%20Mark%20III%20-%2016bit%20%284:3%29.ORF"),
    ("orf-olympus-xz2.orf", "https://raw.pixls.us/getfile.php/1432/nice/Olympus%20-%20XZ-2%20-%2012bit%20%284:3%29.orf"),
    ("pef-pentax-k10d.pef", "https://raw.pixls.us/getfile.php/2239/nice/Pentax%20-%20K10D%20-%2012bit%2012bit%20compressed%20%283:2%29.PEF"),
    ("pef-pentax-k3.pef", "https://raw.pixls.us/getfile.php/1075/nice/Pentax%20-%20K-3%20-%2014bit%20%283:2%29.PEF"),
    ("pef-pentax-k5iis.pef", "https://raw.pixls.us/getfile.php/1198/nice/Pentax%20-%20K-5%20II%20s%20-%2014bit%20%283:2%29.PEF"),
    (
        "raf-fuji-xa2-12bit-bayer.raf",
        "https://raw.pixls.us/getfile.php/2883/nice/Fujifilm%20-%20X-A2%20-%2012bit%2012bit%20uncompressed%20%283:2%29.RAF",
    ),
    (
        "raf-fuji-xa5-14bit-bayer.raf",
        "https://raw.pixls.us/getfile.php/2526/nice/Fujifilm%20-%20X-A5%20-%2014bit%2014bit%20uncompressed%20%283:2%29.RAF",
    ),
    ("raf-fuji-xe1-12bit.raf", "https://raw.pixls.us/getfile.php/3098/nice/Fujifilm%20-%20X-E1%20-%2012bit%2012bit%20uncompressed%20%283:2%29.RAF"),
    ("raf-fuji-xt20-14bit.raf", "https://raw.pixls.us/getfile.php/1177/nice/Fujifilm%20-%20X-T20%20-%2014bit%2014bit%20uncompressed%20%283:2%29.RAF"),
    (
        "raf-fuji-xt20-compressed.raf",
        "https://raw.pixls.us/getfile.php/1178/nice/Fujifilm%20-%20X-T20%20-%2014bit%2014bit%20compressed%20%283:2%29.RAF",
    ),
    ("raw-panasonic-fz50.raw", "https://raw.pixls.us/getfile.php/2234/nice/Panasonic%20-%20DMC-FZ50%20-%204:3.RAW"),
    ("raw-panasonic-fz8.raw", "https://raw.pixls.us/getfile.php/2282/nice/Panasonic%20-%20DMC-FZ8%20-%204:3.RAW"),
    ("rw2-panasonic-fz1000m2-4x3.rw2", "https://raw.pixls.us/getfile.php/4706/nice/Panasonic%20-%20DC-FZ10002%20-%204:3.RW2"),
    // Fujifilm predictive compression: older X-Trans, 40 MP X-Trans, 14/16-bit GFX; both compression modes.
    (
        "raf-fuji-gfx100-3773.raf",
        "https://raw.pixls.us/getfile.php/3773/nice/Fujifilm%20-%20GFX%20100%20-%2016bit%2016bit%20compressed%20%284:3%29.RAF",
    ),
    (
        "raf-fuji-gfx100-3775.raf",
        "https://raw.pixls.us/getfile.php/3775/nice/Fujifilm%20-%20GFX%20100%20-%2014bit%2014bit%20compressed%20%284:3%29.RAF",
    ),
    ("raf-fuji-gfx100rf-8091.raf", "https://raw.pixls.us/getfile.php/8091/nice/Fujifilm%20-%20GFX100RF%20-%2016bit%20compressed%20%284:3%29.RAF"),
    (
        "raf-fuji-gfx100s-4495.raf",
        "https://raw.pixls.us/getfile.php/4495/nice/Fujifilm%20-%20GFX100S%20-%2016bit%2016bit%20lossless%20compressed%20%284:3%29.RAF",
    ),
    (
        "raf-fuji-gfx100s-4503.raf",
        "https://raw.pixls.us/getfile.php/4503/nice/Fujifilm%20-%20GFX100S%20-%2016bit%2016bit%20compressed%20%284:3%29.RAF",
    ),
    (
        "raf-fuji-gfx50s-1435.raf",
        "https://raw.pixls.us/getfile.php/1435/nice/Fujifilm%20-%20GFX%2050S%20-%2014bit%2014bit%20compressed%20%283:2%29.RAF",
    ),
    ("raf-fuji-xe5-8509.raf", "https://raw.pixls.us/getfile.php/8509/nice/Fujifilm%20-%20X-E5%20-%2014bit%20lossy%20compressed%20%283:2%29.RAF"),
    ("raf-fuji-xh2-6001.raf", "https://raw.pixls.us/getfile.php/6001/nice/Fujifilm%20-%20X-H2%20-%2014bit%2014bit%20compressed%20%283:2%29.RAF"),
    ("raf-fuji-xh2-6002.raf", "https://raw.pixls.us/getfile.php/6002/nice/Fujifilm%20-%20X-H2%20-%2014bit%2014bit%20compressed%20%283:2%29.RAF"),
    ("raf-fuji-xm5-7748.raf", "https://raw.pixls.us/getfile.php/7748/nice/Fujifilm%20-%20X-M5%20-%2014bit%20compressed%20%283:2%29.RAF"),
    ("raf-fuji-xt2-865.raf", "https://raw.pixls.us/getfile.php/865/nice/Fujifilm%20-%20X-T2%20-%2014bit%2014bit%20compressed%20%283:2%29.RAF"),
    (
        "raf-fuji-xt4-3914.raf",
        "https://raw.pixls.us/getfile.php/3914/nice/Fujifilm%20-%20X-T4%20-%2014bit%2014bit%20lossless%20compressed%20%283:2%29.RAF",
    ),
    ("raf-fuji-xt4-3918.raf", "https://raw.pixls.us/getfile.php/3918/nice/Fujifilm%20-%20X-T4%20-%2014bit%2014bit%20compressed%20%283:2%29.RAF"),
    ("raf-fuji-xt5-6122.raf", "https://raw.pixls.us/getfile.php/6122/nice/Fujifilm%20-%20X-T5%20-%2014bit%2014bit%20compressed%20%283:2%29.RAF"),
    ("raf-fuji-xt5-6123.raf", "https://raw.pixls.us/getfile.php/6123/nice/Fujifilm%20-%20X-T5%20-%2014bit%2014bit%20compressed%20%283:2%29.RAF"),
    ("raf-fuji-xt50-7807.raf", "https://raw.pixls.us/getfile.php/7807/nice/Fujifilm%20-%20X-T50%20-%2014bit%20compressed%20%283:2%29.RAF"),
    ("rw2-panasonic-g9-b.rw2", "https://raw.pixls.us/getfile.php/2348/nice/Panasonic%20-%20DC-G9%20-%204:3.RW2"),
    ("rw2-panasonic-g9.rw2", "https://raw.pixls.us/getfile.php/2585/nice/Panasonic%20-%20DC-G9%20-%204:3.RW2"),
    ("rw2-panasonic-gh1.rw2", "https://raw.pixls.us/getfile.php/1323/nice/Panasonic%20-%20DMC-GH1%20-%204:3.RW2"),
    ("rw2-panasonic-gh5.rw2", "https://raw.pixls.us/getfile.php/1517/nice/Panasonic%20-%20DC-GH5%20-%204:3.RW2"),
    ("rw2-panasonic-gh5m2.rw2", "https://raw.pixls.us/getfile.php/5082/nice/Panasonic%20-%20DC-GH5M2%20-%204:3.RW2"),
    ("rw2-panasonic-gh5s.rw2", "https://raw.pixls.us/getfile.php/2603/nice/Panasonic%20-%20DC-GH5S%20-%204:3.RW2"),
    ("rw2-panasonic-gh6.rw2", "https://raw.pixls.us/getfile.php/5876/nice/Panasonic%20-%20DC-GH6%20-%204:3.RW2"),
    ("rw2-panasonic-gx80.rw2", "https://raw.pixls.us/getfile.php/1569/nice/Panasonic%20-%20DMC-GX80%20-%204:3.RW2"),
    ("rw2-panasonic-s1.rw2", "https://raw.pixls.us/getfile.php/3038/nice/Panasonic%20-%20DC-S1%20-%203:2.RW2"),
    ("rw2-panasonic-s5-format7.rw2", "https://raw.pixls.us/getfile.php/6339/nice/Panasonic%20-%20DC-S5%20-%203:2.RW2"),
    ("rw2-panasonic-s5m2.rw2", "https://raw.pixls.us/getfile.php/7790/nice/Panasonic%20-%20DC-S5M2%20-%2014bit%20%283:2%29.RW2"),
    ("rw2-panasonic-s9.rw2", "https://raw.pixls.us/getfile.php/7702/nice/Panasonic%20-%20DC-S9%20-%203:2.RW2"),
    ("rwl-leica-dlux7.rwl", "https://raw.pixls.us/getfile.php/4204/nice/Leica%20-%20D-Lux%207%20-%204:3.RWL"),
];
/// SHA-256 pins for every Canon CR3 in the corpus: the complete raw.pixls.us Canon data tree (110 files over
/// 29 bodies, swept 2026-10-09), keyed by the `corpus/raw/` file name. `cargo xtask corpus --download` verifies
/// each file against its pin and fails on content drift. The six `getfile.php` entries above are pinned by the
/// hash of the data-tree file they were verified byte-identical to.
const CR3_SHA256: &[(&str, &str)] = &[
    ("cr3-canon-eos-m200-mg-2231.cr3", "9b900c04ae37ba827b7894e66907657a0b174a74855c40039a8ca0c95583f141"),
    ("cr3-canon-eos-m200-mg-2233.cr3", "0585a468143c6606936d0ffbff1788c6c02e3610d8ecb50614e21492fbacd052"),
    ("cr3-canon-eos-r10-img-6310.cr3", "2dc0bbdf04c93ca62c914604f644c735286037912d6fba609d0cb78d00acd097"),
    ("cr3-canon-eos-r50-v-img-0034.cr3", "955ff98710505498ce7f98cbcc5875793ef2d6941b3b5692ed3baf2817ee7c9f"),
    ("cr3-canon-eos-r5m2-aps-c-craw.cr3", "6a597497776a6abbfab8fcf12d0086ac7a3f8afea2879d61accf6bbf48d357d7"),
    ("cr3-canon-eos-r5m2-aps-c-raw.cr3", "9f88c0ff012924193b7f4497559d133dfe626766d600b5d67689156721739ead"),
    ("cr3-canon-eos-r5m2-craw.cr3", "b3c08a5c7a97299dfd58896a12b939e7b43ec78682761812524825e6e5d087be"),
    ("cr3-canon-eos-r5m2-raw.cr3", "d6e6aa3b493ea8936dff0efb0cc17c87471437e7317bce37c8c82ba08b9cab7b"),
    ("cr3-canon-eos-r6-mark-iii-img-4241.cr3", "e491e4bb960961b5fa299361bf698310a80cbe7b15d30d8dad3bb21bc5457dab"),
    ("cr3-canon-eos-r6-mark-iii-img-4242.cr3", "1395adbae77aaf0ea344fbe2eb2eb7ff825ccae815177cc499fb65e55eaaa495"),
    ("cr3-canon-eos-r6-mark-iii-img-4243.cr3", "1f533686b2230817dac98266a6cc8ffd960413c109965768b7efc05cc1ca96eb"),
    ("cr3-canon-eos-r6-mark-iii-img-4244.cr3", "7e1a5d84297a52853b0bbe5a5c03a5d4f5b93e00b7020271523240f9cabfe50e"),
    ("cr3-canon-m50-craw.cr3", "15384b775867ec4c42b11882837f1e368cedc0561832ffab271221e6bb80be4c"),
    ("cr3-canon-m50-raw.cr3", "1ba9ad6b51b315b88820eb0fe5fd7f52bc136ba37b1a5ca76825ebfe25b18058"),
    ("cr3-canon-powershot-v1-img-1225.cr3", "0a0abfd81b3a34fa513ac8ed24c809275160d65b07e892c2eb4353b77c535a31"),
    ("cr3-canon-r100-craw.cr3", "0b66842b2fe00329ebc05fe8ab9357ddbbe1a0a2d2e69a2893102de7fdf5c942"),
    ("cr3-canon-r100-raw.cr3", "6d83217d58a5e6d2dabcf470430e91a16676b60531c025b81fa7453fc74b0521"),
    ("cr3-canon-r8-craw.cr3", "df33cf394573645ce03dea1b2e9f0b5cc2b7734e9e3391ee7114151414cf2812"),
    ("cr3-canon-r8-raw.cr3", "7d5c6dbb11ff7e6ee58715d90a20f5d801e8b103f4711ff6ea329ceafcef254c"),
    (
        "cr3-eos-1d-x-mark-iii-canon-eos-1d-x-mark-iii-craw-iso-100-nocrop-nodual.cr3",
        "e5755d1414a2dd51fb0120b272058f00b0199861646070c00304d927eb119560",
    ),
    (
        "cr3-eos-1d-x-mark-iii-canon-eos-1d-x-mark-iii-raw-iso-100-nocrop-nodual.cr3",
        "8f38b9de90eaebefcd54022fcbfdf38e05fb86831c4c976c8ac05c599e63535b",
    ),
    ("cr3-eos-200d-mark-ii-mg-3608.cr3", "4ea0cbd52b760a5654d3f431c7c9ecf8acdc62a6aa74a2cfbfe651693c740c77"),
    ("cr3-eos-250d-canon250d-craw-1-1.cr3", "7ece52df8b7ed868e6c6096e6a28f807694768cc045383c541e9707f1a669a5d"),
    ("cr3-eos-250d-canon250d-craw-16-9.cr3", "e81963db1ee6df76bb10fd536b3646cede277f2ca8b26895b1b34debafa03fc4"),
    ("cr3-eos-250d-canon250d-craw-3-2.cr3", "69bdd267defb3d676f41f62a5e5ea65651669a04191cf4e89740917c59cedbcc"),
    ("cr3-eos-250d-canon250d-craw-4-3.cr3", "b55eb1a78810ebbfbed5c9a2639a739fee10e93e03f67d239a4708062603de2a"),
    ("cr3-eos-250d-canon250d-raw-1-1.cr3", "1af771b745999f058c89d70514a154007480bdcea0cd6ca7db14f65e6a422a6b"),
    ("cr3-eos-250d-canon250d-raw-19-9.cr3", "fc07c0e502bd9d945e510b33d1c20b5b996a487547a642a218b0660a499ec09a"),
    ("cr3-eos-250d-canon250d-raw-2-3.cr3", "ed1bd1bb8dbcf31735fbe52eeabdbd3a32236eda9cb20b132e8847cb0d89517f"),
    ("cr3-eos-250d-canon250d-raw-4-3.cr3", "3ab8d63285f9a5b5c5d76641847cdb14162a7d2968d78f597aedb2e08e902012"),
    ("cr3-eos-850d-img-1837.cr3", "47f7747b74073515bdbd65015738bc31947504d68255c5377baa21f179df2486"),
    ("cr3-eos-850d-img-1906.cr3", "c6e781a58fdbe97fac5b99deb878a8568904aba48de0e4c31076b4d3887e2c91"),
    ("cr3-eos-90d-canon-90d-craw-iso-100.cr3", "c8c918b2d1a53e64d7d5009ebac89a920dbba5f766c2f396bb4ec95312b21034"),
    ("cr3-eos-90d-canon-90d-raw-iso-100.cr3", "e47981d90142291a2bd7ba5a3dea51a25fff72ad3dad83dd2d2698f3ef5f59a8"),
    ("cr3-eos-kiss-m-img-2695.cr3", "49a4bcd16e0236451b73ae052af8a4d0c9872063f79fc62fdf982d462792741c"),
    ("cr3-eos-m50-canon-eos-m50-craw-iso-100-nocrop-nodual.cr3", "27a88fdae82516ba136e12c2cc9b934f25dbaaac24d64ab4e5517f8524463594"),
    ("cr3-eos-m50-mark-ii-img-0051.cr3", "1a3c11d85f48b8c644664b505f69fbdd75eb51270bb8ab556dbc660f0487e957"),
    ("cr3-eos-m6-mark-ii-img-craw-1-1.cr3", "0ccbb6c447d0fab996291be9a5c2728cd7367bcc2f640e2a30eae112e13e6c0e"),
    ("cr3-eos-m6-mark-ii-img-craw-16-9.cr3", "2f766ce3b220a16430be3bcca96da58e3567cc7a04d7c46a18770f56b5a70dd1"),
    ("cr3-eos-m6-mark-ii-img-craw-3-2.cr3", "94635c6f91e7d743cee6881cc2519992ae9f9031fd86b49e5d0f16edf5961b55"),
    ("cr3-eos-m6-mark-ii-img-craw-4-3.cr3", "9bb482b003a0749af615a9b3e52616b01a2ad2794a2d9fe47fafcf6bdcb9a35a"),
    ("cr3-eos-m6-mark-ii-img-raw-1-1.cr3", "6a7d52f7ae5e12b58cfebd39d82e8d71f35e6c6216775569eea7a7e168f83fac"),
    ("cr3-eos-m6-mark-ii-img-raw-16-9.cr3", "4f89f3770c794a0ca5af86b84f1f694eb5a0f553b64f85fc1ba3ddf412a211b8"),
    ("cr3-eos-m6-mark-ii-img-raw-3-2.cr3", "48be4fbcc36e4f9bf31dd63ba7f97aa906a39b29f6b2caa679506c4cdb20f5a7"),
    ("cr3-eos-m6-mark-ii-img-raw-4-3.cr3", "8b41dbeadb91c44e596da5fef935fe3acb2154c06afb11123b3079da79a5ec51"),
    ("cr3-eos-r-canon-eos-r-craw-iso-100-crop-dual.cr3", "c1f07fa57aa76290df640a15f2a1a74deb21519b0c99051941c47b36c685246d"),
    ("cr3-eos-r-canon-eos-r-craw-iso-100-crop-nodual.cr3", "90bbf397b592f9ce9799477df4affbdf0025578c07eac26f928696b0d40d5121"),
    ("cr3-eos-r-canon-eos-r-craw-iso-100-dual-pixel-on.cr3", "cfd118af4477ef37538eb953421572a7a5c80694e59764049223107cc0c9ae83"),
    ("cr3-eos-r-canon-eos-r-craw-iso-100.cr3", "1b3efe0664a28a0decb53e225cbfecdbc0dd27d85abdc4683fc2a4add5129b4d"),
    ("cr3-eos-r-canon-eos-r-raw-iso-100-crop-dual.cr3", "08ae87dd40cf051d2073d0eb2309548297ce8780e0d956bd766fe36dc561d7d1"),
    ("cr3-eos-r-canon-eos-r-raw-iso-100-crop-nodual.cr3", "78ef0efef541c2618b9f95915e825726fc31b06f7d61149167f5af620f6b4c30"),
    ("cr3-eos-r-canon-eos-r-raw-iso-100-dual-pixel-on.cr3", "4b0809c4fca8c00d84df0a1b0ebbe4959784e50bf865616f151e67d76a9e4b01"),
    ("cr3-eos-r-canon-eos-r-raw-iso-100.cr3", "89bd55532b0cceb5efa360e97827ba35c2076bce0de4675f3a4413a883dac880"),
    ("cr3-eos-r10-img-4276.cr3", "5d2e076d38e7835d4c4f13dc2030a913cc573a31d7199b60777d23a25ccc3ef9"),
    ("cr3-eos-r10-img-4279.cr3", "dd33ee51dfcb142e8954e1ce375d60786459b4194968831e65d62d86d2c0a8cb"),
    ("cr3-eos-r10-img-4361.cr3", "15d017f0770b3da17180077617ffe07b786a1ec2cb36297f81a82a27a54756db"),
    ("cr3-eos-r3-c3a1200.cr3", "295d46a4d5383895e63126f67000d4352a5bb425ee4d8b16eb6b6c284bdf7994"),
    ("cr3-eos-r5-canon-eos-r5-craw-iso-100-crop-dual.cr3", "88497fe7f71018ce35810e6b033fb696c7792b57e8c73e842d8326095287526a"),
    ("cr3-eos-r5-canon-eos-r5-craw-iso-100-crop-nodual.cr3", "a5f4935e0331cc16003c0b1ab6c657272739e82175557aaae576e78a8262511b"),
    ("cr3-eos-r5-canon-eos-r5-craw-iso-100-nocrop-dual.cr3", "00b80633af35c0f85b0299a1d2ea87c6788a9007a1df8a3c5c05ea487b2549e1"),
    ("cr3-eos-r5-canon-eos-r5-craw-iso-100-nocrop-nodual.cr3", "2deae46b816e56ba91ae7a3892ca121290e12d5b22afb11f5b05a3d8f09f759f"),
    ("cr3-eos-r5-canon-eos-r5-raw-iso-100-crop-dual.cr3", "10a1116bd00e92314b09382b403f76706b1eeab3ce33234a6c75fe25ed289756"),
    ("cr3-eos-r5-canon-eos-r5-raw-iso-100-crop-nodual.cr3", "dc3ef975ef029fe527ba6cafa0f899720cfdcbcfb4f590edcff81f26d499c2f0"),
    ("cr3-eos-r5-canon-eos-r5-raw-iso-100-nocrop-dual.cr3", "d52fca2f9233211d7912632f0f27b7284807ed3262627bfce7e4f12cf343820e"),
    ("cr3-eos-r5-canon-eos-r5-raw-iso-100-nocrop-nodual.cr3", "21430c36387efe65bd09ac0fcd724bfd959d93dd391ec811a1b741c8663becb5"),
    ("cr3-eos-r50-canon-r50-iso100-craw-electronicshutter.cr3", "38fc458f2f74d0cfa04afd0ce513b40797080079f0786bedbfc55c39cc28d7d8"),
    ("cr3-eos-r50-canon-r50-iso100-craw-mechanicshutter.cr3", "b79af9ffa184dd740e8af7a9f451f7150053d7e456b3435ca2e43bbbf5f65026"),
    ("cr3-eos-r50-canon-r50-iso100-raw-electronicshutter.cr3", "979d27717906ce0d7b66132a68eb01cf63d22388de1808239ee65991184540c5"),
    ("cr3-eos-r50-canon-r50-iso100-raw-mechanicshutter.cr3", "cbc0666f27d37007203df53f3668c30d3980aab93c3e33e7bff41c72b77414e9"),
    ("cr3-eos-r6-canon-eos-r6-craw-iso-100-crop-nodual.cr3", "74abb0a113d075ad9887a058082f40dd2a938c4813a08474d82356f11a027778"),
    ("cr3-eos-r6-canon-eos-r6-craw-iso-100-nocrop-nodual.cr3", "4775df76433b1e5844bc7a42b95d93efc8438c0a4cd9681e120e23fe3e3007e8"),
    ("cr3-eos-r6-canon-eos-r6-raw-iso-100-crop-nodual.cr3", "6156ee9cfd1a61e08719a8309095d05162891fd2adb8dd1505b31d4716dcee05"),
    ("cr3-eos-r6-canon-eos-r6-raw-iso-100-nocrop-nodual.cr3", "81fab349ae9080fd0172583b7fd5ea36806e95ffa434909d2ccc6527dde34ee6"),
    ("cr3-eos-r6-mark-ii-canon-r6m2-img-0346-raw-normal-full.cr3", "1a93f96556aabbc4a7260750ab0a191ec7926bb978fe8428bf3465121da730d6"),
    ("cr3-eos-r6-mark-ii-canon-r6m2-img-0347-raw-normal-crop.cr3", "b9ff8d98be99bb22effe47c61906dadcb214b0909699cb4fa1ff089b2f889a07"),
    ("cr3-eos-r6-mark-ii-canon-r6m2-img-0349-raw-dual-crop.cr3", "6a82fe3b643ba42c96655108734999599c1c2f34e463951af7cc4b48325f7332"),
    ("cr3-eos-r6-mark-ii-canon-r6m2-img-0351-raw-dual-full.cr3", "9ecb9254ff3873091a7f69e5af8509547a4d2cda3c715c1ddb4397067fd4ebc4"),
    ("cr3-eos-r6-mark-ii-canon-r6m2-img-0355-craw-dual-full.cr3", "18a3ce780ed5433dc9b078f94bb8bd87927b0877331cce9597078f0dd4313c9f"),
    ("cr3-eos-r6-mark-ii-canon-r6m2-img-0356-craw-dual-crop.cr3", "deed19ea7b5178c70f64710c97bcdc9d21f6610c19b42a2310223e51034d4fc8"),
    ("cr3-eos-r6-mark-ii-canon-r6m2-img-0359-craw-normal-crop.cr3", "51ae1e673d2801d8a54f33dd8669c43034c506e2d355e75b26c0648e2f14a431"),
    ("cr3-eos-r6-mark-ii-canon-r6m2-img-0361-craw-normal-full.cr3", "409eb724212abde94e18fbd3170abf3264951fab13384c36cbf6fb4f22ea4201"),
    ("cr3-eos-r7-443a0157.cr3", "887439cb1a45becd6a5c85fe75ae10e6c520a9f13a8b9ed077c6cf5d7c37700c"),
    ("cr3-eos-r7-443a0159.cr3", "10a18e3f01ca9cd93408496d3853388ef140830548bfeed4237e40f967eb9d5c"),
    ("cr3-eos-r7-443a0161.cr3", "c01cee1b598120668370892c16a73da915f2aaec8169c5c60a96efd98d9aa220"),
    ("cr3-eos-r7-443a0163.cr3", "b87079049afda620c88717e4b895bc80646428b41795f43f10ac0cc06ad0d496"),
    ("cr3-eos-r8-craw-full-frame-dual-pixel.cr3", "f2caa26172e94ddb893c6e1a594c23b95b8d3197a5e7243f84ed4b0ab2f682cd"),
    ("cr3-eos-r8-raw-full-frame-dual-pixel.cr3", "c1dac426c6e3cba0409e13ab65e22639d6831792685dda2de39b4e50a68531f8"),
    ("cr3-eos-rebel-sl3-sedona2020-sl3-mg-1061.cr3", "6f26a2e4ea5a3e3b859662c9b01da6b24731413835aa601065bff1d7f30c6092"),
    ("cr3-eos-rp-canon-eos-rp-craw-iso-100-crop-nodual.cr3", "4a859ab137f792aedaf3416b0aad1497ebaab02052a49453902e226699154aa7"),
    ("cr3-eos-rp-canon-eos-rp-craw-iso-100-nocrop-nodual.cr3", "bd3270610593d37ff8a399908bb9406a4e32f9151d55bf19ac1cf8f10d787b46"),
    ("cr3-eos-rp-canon-eos-rp-raw-iso-100-crop-nodual.cr3", "7d4ba45dc9264d598336133593c677b09774bcdf13b602ee770e72415a652a16"),
    ("cr3-eos-rp-canon-eos-rp-raw-iso-100-nocrop-nodual.cr3", "dcbd0f6f9929de2e2331b0fd10a9d01e7cfd04c5a5e4038456de1647d714cdf2"),
    ("cr3-powershot-g5-x-mark-ii-canon-powershot-g5-x-mark-ii-craw-1-1.cr3", "7d1160f4c7159dd30daae7a99a575d0c37dc11493aa6c796e1ff9e504c2bdd8f"),
    ("cr3-powershot-g5-x-mark-ii-canon-powershot-g5-x-mark-ii-craw-16-9.cr3", "71b2f703c09be153e48ff0075772f19ea0a2c2e81acc08d11e0a979ba79f07ce"),
    ("cr3-powershot-g5-x-mark-ii-canon-powershot-g5-x-mark-ii-craw-3-2.cr3", "16d7b944344004c866bf2f84c5673969e76adfe16e5fb4388da48c72bdd58058"),
    ("cr3-powershot-g5-x-mark-ii-canon-powershot-g5-x-mark-ii-craw-4-3.cr3", "53eec9eecf1a7270b06750efe73f90712d5649ab50e41a7310d37c3097689e54"),
    ("cr3-powershot-g5-x-mark-ii-canon-powershot-g5-x-mark-ii-raw-1-1.cr3", "13bc9926954dd62f2b84c1539428554754c078953c7af9baa5650cb85237aec8"),
    ("cr3-powershot-g5-x-mark-ii-canon-powershot-g5-x-mark-ii-raw-16-9.cr3", "feb0ece1244ba45f179f48ad1a52c887e6042a605927156d9b944d1305f10d59"),
    ("cr3-powershot-g5-x-mark-ii-canon-powershot-g5-x-mark-ii-raw-3-2.cr3", "16f69a73e25e0555259a942d65be816042dcb3af03505b25335e5d02ad6f2af6"),
    ("cr3-powershot-g5-x-mark-ii-canon-powershot-g5-x-mark-ii-raw-4-3.cr3", "322c8e0765513e94071fc84fa29c39ba6cbd5fd70be1a657d7d7fe55dbd54fc9"),
    ("cr3-powershot-g7-x-mark-iii-img-0253.cr3", "63ce623055dd2ac7374e0b1c645a4eabc7e181cd12c2039625f64b4e63a5ee86"),
    ("cr3-powershot-g7-x-mark-iii-img-0254.cr3", "29fa26718f2791f8dc3a69d8517d0575f8cf165492253671e7b1a5151a512f02"),
    ("cr3-powershot-sx70-hs-img-1000.cr3", "3b346078836f933f505338ba45965be17a39cac82bfffc7006db07e3af944839"),
    ("cr3-powershot-sx70-hs-img-1001.cr3", "26d5f6deebb65e5a544d7d1b7706dca2bb8b06642065aa725c1394054e9b717d"),
    ("cr3-powershot-sx70-hs-img-1002.cr3", "1b2b52b08f8a0701fccd6c9371cd5160105db8b6cbcd0b8e8ada47d680aac30c"),
    ("cr3-powershot-sx70-hs-img-1003.cr3", "ca34fd5a464ddcca13ff66376da685630045e82050af3136570c7f804bc130c8"),
    ("cr3-powershot-sx70-hs-img-1004.cr3", "ab5a2da7ee80c2ef2ae05bda30b0e70c1fad81e81f6d1994aeb520b83ef662c1"),
    ("cr3-powershot-sx70-hs-img-1005.cr3", "5c37f0d865c79e22156f684281773dc4bf550f8ae1bfce2acc23cacb07a7893c"),
    ("cr3-powershot-sx70-hs-img-1006.cr3", "fad9653d691d002bc2de4f1e98366e6031351412fd45523d2cd753fbc4645e62"),
    ("cr3-powershot-sx70-hs-img-1007.cr3", "fef71beb2df8bd94ccf2bd9783e8d696a59f71cce494f94aca4c122fed911e1a"),
];

/// SHA-256 (FIPS 180-4), std-only so the corpus manifest stays dependency-free.
fn sha256_hex(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be,
        0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa,
        0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85,
        0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3,
        0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f,
        0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut h = [0x6a09e667u32, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());
    for block in msg.as_chunks::<64>().0 {
        let mut w = [0u32; 64];
        for (i, word) in block.as_chunks::<4>().0.iter().enumerate() {
            w[i] = u32::from_be_bytes(*word);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let mut v = h;
        for i in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
            let t1 = v[7].wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(maj);
            v = [t1.wrapping_add(t2), v[0], v[1], v[2], v[3].wrapping_add(t1), v[4], v[5], v[6]];
        }
        h = std::array::from_fn(|i| h[i].wrapping_add(v[i]));
    }
    h.iter().map(|word| format!("{word:08x}")).collect()
}

/// Verify every CR3 under `dest` against its pinned SHA-256. Content drift is an error; absent files are
/// expected when the corpus has not been downloaded yet and are only reported.
fn verify_corpus_hashes(dest: &std::path::Path) -> Result<(), String> {
    let (mut verified, mut absent) = (0usize, 0usize);
    let mut drift = Vec::new();
    for (name, want) in CR3_SHA256 {
        let path = dest.join(name);
        if !path.exists() {
            absent += 1;
            continue;
        }
        let bytes = std::fs::read(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
        let got = sha256_hex(&bytes);
        if got == *want {
            verified += 1;
        } else {
            drift.push(format!("{}: sha256 {got} != pinned {want}", path.display()));
        }
    }
    if drift.is_empty() {
        println!("  CR3 pins: {verified} verified against SHA-256, {absent} absent");
        Ok(())
    } else {
        Err(format!(
            "{} CR3 file(s) do not match their pinned SHA-256 (delete the file and re-run with --download):
  {}",
            drift.len(),
            drift.join(
                "
  "
            )
        ))
    }
}

fn cmd_corpus(download: bool) -> Result<(), String> {
    let corpus = root().join("corpus");
    println!(
        "Test corpora live under {} (git-ignored, never committed).
Tests that use a corpus skip cleanly when it is absent.

  corpus/raw/        raw.pixls.us samples (CC0) — lightcraft-raw decodes every file.
  corpus/images/     CC0 / public-domain JPEG/PNG/TIFF/HEIC samples (optional)
",
        corpus.display()
    );
    if !download {
        return Ok(());
    }
    let dest = corpus.join("raw");
    std::fs::create_dir_all(&dest).map_err(|e| format!("create {}: {e}", dest.display()))?;
    for (name, url) in RAW_SAMPLES {
        let out = dest.join(name);
        if out.exists() {
            continue;
        }
        let mut curl = Command::new("curl");
        curl.args(["-fsSL", "-o"]).arg(&out).arg(url);
        run(curl, &format!("curl {url}"))?;
    }
    verify_corpus_hashes(&dest)
}

#[cfg(test)]
mod ci_jobs_tests {
    use super::ci_jobs;

    #[test]
    fn jobs_follow_available_ram_and_never_exceed_the_cpus() {
        assert_eq!(ci_jobs(Some(3 * 1024), 8), 2, "a 4 GB machine with 3 GB free");
        assert_eq!(ci_jobs(Some(6 * 1024), 8), 4, "an 8 GB machine with 6 GB free");
        assert_eq!(ci_jobs(Some(7 * 1024), 32), 4, "a busy 32 GB machine: what is free counts");
        assert_eq!(ci_jobs(Some(24 * 1024), 32), 16);
        assert_eq!(ci_jobs(Some(64 * 1024), 8), 8, "capped at the CPU count");
        assert_eq!(ci_jobs(Some(500), 8), 1, "at least one");
        assert_eq!(ci_jobs(None, 32), 4, "unknown RAM keeps the old default");
        assert_eq!(ci_jobs(None, 2), 2);
    }
}

#[cfg(test)]
mod corpus_tests {
    use super::{CR3_SHA256, RAW_SAMPLES, sha256_hex};

    #[test]
    fn sha256_matches_the_fips_vectors() {
        assert_eq!(sha256_hex(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(
            sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    #[test]
    fn every_corpus_cr3_entry_is_sha256_pinned_and_vice_versa() {
        let mut entries = RAW_SAMPLES.iter().filter(|(n, _)| n.starts_with("cr3-")).map(|(n, _)| *n).collect::<Vec<_>>();
        let mut pins = CR3_SHA256.iter().map(|(n, _)| *n).collect::<Vec<_>>();
        entries.sort_unstable();
        pins.sort_unstable();
        assert_eq!(entries.len(), 110, "expected the complete raw.pixls.us Canon CR3 tree (110 files)");
        assert_eq!(entries, pins, "cr3- entries in RAW_SAMPLES and CR3_SHA256 pins must cover the same files");
    }

    #[test]
    fn cr3_pins_are_lowercase_hex64_and_their_urls_are_pinned_hosts() {
        for (name, hash) in CR3_SHA256 {
            assert_eq!(hash.len(), 64, "{name}");
            assert!(hash.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)), "{name}: {hash}");
        }
        for (name, url) in RAW_SAMPLES.iter().filter(|(n, _)| n.starts_with("cr3-")) {
            assert!(url.starts_with("https://raw.pixls.us/"), "{name}: {url}");
        }
    }
}
