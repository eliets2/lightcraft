//! Deterministic local smoke fuzzer — runs the same entry functions as the
//! cargo-fuzz targets, with plain-std mutations, on any platform and toolchain
//! (libFuzzer does not run on windows-gnu, so this is how Windows and stable
//! machines smoke-test today; Linux CI runs the real thing, see ../README.md).
//!
//! The loop is fully reproducible: a finding prints the RNG seed and case
//! number, and re-running with the same `--seed` replays the exact sequence.
//!
//! Usage:
//!   cargo run --release -p lightcraft-fuzz --bin mutate-smoke -- \
//!       [--minutes 5] [--ops 0] [--seed 0] [--out fuzz/artifacts-smoke] \
//!       TARGET SEED_PATH...
//!
//! TARGET is one of cr3_container | crx_bitstream | cr3_whole_file |
//! iso_bmff_boxes; SEED_PATH is a file or a directory (walked recursively).
//! Exits 1 if any seed or mutated input panics; hangs cannot be caught inside
//! one process, so cases slower than 5 s are reported as slow findings.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

const DEFAULT_MINUTES: u64 = 5;
const SLOW_CASE: Duration = Duration::from_secs(5);
const MAX_SEED_BYTES: u64 = 64 * 1024 * 1024;

fn main() -> ExitCode {
    let mut minutes = DEFAULT_MINUTES;
    let mut ops: u64 = 0;
    let mut rng_seed: u64 = 0;
    let mut out = PathBuf::from("fuzz/artifacts-smoke");
    let mut rest = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--minutes" => match args.next().and_then(|v| v.parse().ok()) {
                Some(v) => minutes = v,
                None => return fail("--minutes needs a number"),
            },
            "--ops" => match args.next().and_then(|v| v.parse().ok()) {
                Some(v) => ops = v,
                None => return fail("--ops needs a number"),
            },
            "--seed" => match args.next().and_then(|v| v.parse().ok()) {
                Some(v) => rng_seed = v,
                None => return fail("--seed needs a number"),
            },
            "--out" => match args.next() {
                Some(v) => out = PathBuf::from(v),
                None => return fail("--out needs a value"),
            },
            other => rest.push(other.to_string()),
        }
    }
    let Some(target) = rest.first() else { return fail("usage: mutate-smoke [--minutes N] [--ops N] [--seed S] [--out DIR] TARGET SEED_PATH...") };
    if !lightcraft_fuzz::TARGETS.contains(&target.as_str()) {
        return fail(&format!("unknown target {target}; expected one of {:?}", lightcraft_fuzz::TARGETS));
    }
    if rest.len() < 2 {
        return fail("no seed paths given");
    }

    let mut seeds = Vec::new();
    for path in &rest[1..] {
        if let Err(e) = collect_seeds(Path::new(path), 0, &mut seeds) {
            return fail(&e);
        }
    }
    if seeds.is_empty() {
        return fail("no readable seed files found");
    }

    let mut rng = Rng::new(rng_seed);
    let mut findings = 0;
    println!("smoke {target}: {} seeds (rng seed {rng_seed})", seeds.len());

    // Phase A: replay every seed unmodified — this doubles as the regression
    // run CI does over fuzz/seeds (a panic on a committed seed blocks merge).
    for (index, seed) in seeds.iter().enumerate() {
        if let Some(message) = run_case(target, seed) {
            findings += 1;
            save(&out, target, &format!("seed-{index:04}"), seed);
            eprintln!("FINDING on committed seed {index} ({} bytes): {message}", seed.len());
        }
    }
    println!("phase A: {} seeds replayed, {findings} finding(s)", seeds.len());

    // Phase B: mutation loop until the time or op budget runs out.
    let deadline = Instant::now() + Duration::from_secs(minutes.saturating_mul(60));
    let mut executed: u64 = 0;
    let mut slow = 0;
    let started = Instant::now();
    let mut next_report = Instant::now() + Duration::from_secs(2);
    loop {
        if ops == 0 && Instant::now() >= deadline {
            break;
        }
        if ops > 0 && executed >= ops {
            break;
        }
        let index = rng.below(seeds.len());
        let mut input = seeds[index].clone();
        mutate(&mut rng, &mut input, &seeds);
        let case_started = Instant::now();
        let finding = run_case(target, &input);
        executed += 1;
        match finding {
            Some(message) => {
                findings += 1;
                let name = format!("case-{executed:08}-seed{index:04}");
                save(&out, target, &name, &input);
                eprintln!(
                    "FINDING case {executed} (seed {index}, {} bytes, replay with --seed {rng_seed}): {message}\n  saved to {}/{target}/{name}.bin",
                    input.len(),
                    out.display()
                );
            }
            None if case_started.elapsed() > SLOW_CASE => {
                slow += 1;
                let name = format!("slow-{executed:08}-seed{index:04}");
                save(&out, target, &name, &input);
                eprintln!("SLOW case {executed}: {:.1}s (saved {}/{target}/{name}.bin)", case_started.elapsed().as_secs_f32(), out.display());
            }
            None => {}
        }
        if Instant::now() >= next_report {
            next_report = Instant::now() + Duration::from_secs(2);
            println!(
                "  {} execs, {:.0} exec/s, {findings} finding(s), {slow} slow",
                executed,
                executed as f32 / started.elapsed().as_secs_f32().max(1e-6)
            );
        }
    }
    let secs = started.elapsed().as_secs_f32();
    println!("done: {executed} execs in {secs:.1}s ({:.0} exec/s), {findings} finding(s), {slow} slow", executed as f32 / secs.max(1e-6));
    if findings > 0 { ExitCode::FAILURE } else { ExitCode::SUCCESS }
}

/// Run one case; `Some(message)` when the target panicked.
fn run_case(target: &str, input: &[u8]) -> Option<String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| lightcraft_fuzz::dispatch(target, input))).err().map(|payload| {
        let message = payload
            .downcast_ref::<&str>()
            .map(|s| (*s).to_owned())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "opaque panic payload".to_owned());
        let backtrace = std::backtrace::Backtrace::force_capture();
        format!("PANIC {message}\n{backtrace}")
    })
}

fn save(out: &Path, target: &str, name: &str, input: &[u8]) {
    let dir = out.join(target);
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let _ = std::fs::write(dir.join(format!("{name}.bin")), input);
}

fn collect_seeds(path: &Path, depth: usize, seeds: &mut Vec<Vec<u8>>) -> Result<(), String> {
    if depth > 4 {
        return Ok(());
    }
    if path.is_dir() {
        let entries = std::fs::read_dir(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
        paths.sort();
        for p in paths {
            collect_seeds(&p, depth + 1, seeds)?;
        }
        return Ok(());
    }
    let Ok(metadata) = std::fs::metadata(path) else { return Ok(()) };
    if !metadata.is_file() || metadata.len() > MAX_SEED_BYTES {
        return Ok(());
    }
    match std::fs::read(path) {
        Ok(bytes) => {
            seeds.push(bytes);
            Ok(())
        }
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

// ---------------------------------------------------------------------------
// Mutations (all drawn from the deterministic RNG; no external deps).
// ---------------------------------------------------------------------------

const INTERESTING: [u8; 8] = [0, 1, 0x7f, 0x80, 0xfe, 0xff, 0x0a, 0x20];

fn mutate(rng: &mut Rng, data: &mut Vec<u8>, donors: &[Vec<u8>]) {
    let rounds = 1 + rng.below(4);
    for _ in 0..rounds {
        if data.len() < 8 {
            let n = 1 + rng.below(64);
            data.extend(rng.bytes(n));
            continue;
        }
        match rng.below(9) {
            0 => {
                let at = rng.below(data.len());
                data[at] ^= 1 << rng.below(8);
            }
            1 => {
                let at = rng.below(data.len());
                data[at] = rng.byte();
            }
            2 => {
                // a run of 1–4 "interesting" bytes (box headers love these)
                let at = rng.below(data.len());
                for i in 0..1 + rng.below(4) {
                    if at + i < data.len() {
                        data[at + i] = INTERESTING[rng.below(INTERESTING.len())];
                    }
                }
            }
            3 => {
                let keep = 1 + rng.below(data.len());
                data.truncate(keep);
            }
            4 => {
                let n = 1 + rng.below(256);
                data.extend(rng.bytes(n));
            }
            5 => {
                let at = rng.below(data.len());
                let n = 1 + rng.below(16);
                let bytes = rng.bytes(n);
                data.splice(at..at, bytes);
            }
            6 => {
                // splice a chunk of a random donor over a random range
                let Some(donor) = donors.get(rng.below(donors.len())) else { continue };
                if donor.len() < 16 {
                    continue;
                }
                let start = rng.below(donor.len() / 2);
                let end = (start + rng.below(donor.len() - start)).max(start + 1);
                let at = rng.below(data.len());
                let len = (end - start).min(data.len() - at);
                data[at..at + len].copy_from_slice(&donor[start..start + len]);
            }
            7 => {
                // box-size lie: rewrite a 4-byte size field
                let at = rng.below(data.len().saturating_sub(4));
                let lie = match rng.below(4) {
                    0 => 0u32,                 // to-end-of-file
                    1 => 1,                    // 64-bit size announced
                    2 => 0xffff_fff0,          // enormous
                    _ => rng.below(64) as u32, // far too small
                };
                data[at..at + 4].copy_from_slice(&lie.to_be_bytes());
            }
            _ => {
                // duplicate a chunk of the input to the end
                let start = rng.below(data.len() / 2);
                let end = (start + 1 + rng.below(data.len() - start - 1).max(1)).min(data.len());
                let chunk = data[start..end].to_vec();
                data.extend_from_slice(&chunk);
            }
        }
    }
}

/// SplitMix64: tiny, deterministic, good enough for mutation steering.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed ^ 0x1EC7_B00B_1E5A_1E5A)
    }
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { (self.next() % n as u64) as usize }
    }
    fn byte(&mut self) -> u8 {
        self.next() as u8
    }
    fn bytes(&mut self, n: usize) -> Vec<u8> {
        (0..n).map(|_| self.byte()).collect()
    }
}

fn fail(message: &str) -> ExitCode {
    eprintln!("error: {message}");
    ExitCode::FAILURE
}
