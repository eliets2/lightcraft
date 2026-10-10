//! CR3 decode performance: a printed baseline table plus a two-tier guard (roadmap item 19,
//! "publish the benchmark"; closes D-code-review.md §6 gap 6, "MP/s is printed but never
//! asserted").
//!
//! What is measured: `lightcraft_raw::decode` (container parse + raw sensor decode) on a fixed,
//! representative slice of the CC0 CR3 corpus, bytes pre-read so file IO is excluded, decoded
//! images dropped so no pipeline or demosaicing is included. One warm-up pass precedes the timed
//! passes; timings use the monotonic `std::time::Instant` clock; per-file results report the
//! median wall time and MP/s (full sensor pixels, the same convention as `corpus.rs`).
//!
//! The five files are picked from the xtask corpus manifest (the inline SHA-256 pins in `RAW_SAMPLES`, `xtask/src/main.rs`,
//! which pins every file's SHA-256) to cover the decode paths that dominate real-world cost:
//! the large-sensor lossless tail (EOS R100 RAW — the slowest end-to-end render in the corpus),
//! version-0x100 C-RAW (EOS R8 CRAW), a lossless mid-size sensor (EOS M50 RAW), the biggest
//! sensor with a version-0x200 adaptive-QP C-RAW stream whose QP-map height is odd (EOS R5
//! Mark II CRAW, ~48 MP), and a Dual Pixel C-RAW stream (EOS R5 CRAW, pixel-exact fixture).
//!
//! Two tiers, loudly separated:
//!
//! * **Default (every `cargo test` run, including PR CI):** prints the full table and asserts
//!   only a generous catastrophe floor per file — max(1 MP/s, one tenth of the recorded
//!   baseline MP/s) in release, and (one quarter, one twentieth of the baseline) in the
//!   partially optimized dev profile — so it can never flake under machine load, yet the 10×
//!   entropy-style regression of D-code-review.md §6 fails instead of shipping silently.
//!   Absent corpus files are skipped, like in `corpus.rs`: the default tier is green without
//!   the corpus and never fails on a missing download.
//! * **Nightly gate (`LC_PERF_GATE=1 cargo test --release -p lightcraft-raw --test cr3_perf`),
//!   for the nightly runner only, never PRs:** compares each median MP/s against the committed
//!   baseline in `docs/cr3-perf-baseline.md` (mirrored in `BASELINE_MP_S` below) with a 25%
//!   tolerance, and hard-fails on a corpus file whose SHA-256 no longer matches the manifest.
//!   Baseline numbers are machine-relative: the gate is meaningful only on the machine that
//!   recorded `docs/cr3-perf-baseline.md` (see that file's methodology section).
//!
//! Run with `--release -- --nocapture` to see the table; in gate mode it is printed regardless.

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::Instant;

fn corpus_root() -> PathBuf {
    std::env::var_os("LIGHTCRAFT_CORPUS").map(PathBuf::from).unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus"))
}

/// The representative slice: (file name, SHA-256 as pinned in `RAW_SAMPLES`, what it exercises).
/// Keep the names and hashes in lockstep with the xtask manifest; if a corpus file ever changes,
/// the nightly gate refuses to compare instead of reporting a bogus regression.
const CASES: &[(&str, &str, &str)] = &[
    ("cr3-canon-r100-raw.cr3", "6d83217d58a5e6d2dabcf470430e91a16676b60531c025b81fa7453fc74b0521", "lossless RAW 0x100, the large-sensor slow tail"),
    ("cr3-canon-r8-craw.cr3", "df33cf394573645ce03dea1b2e9f0b5cc2b7734e9e3391ee7114151414cf2812", "C-RAW 0x100 full frame"),
    ("cr3-canon-m50-raw.cr3", "1ba9ad6b51b315b88820eb0fe5fd7f52bc136ba37b1a5ca76825ebfe25b18058", "lossless RAW 0x100, mid-size sensor"),
    (
        "cr3-canon-eos-r5m2-craw.cr3",
        "b3c08a5c7a97299dfd58896a12b939e7b43ec78682761812524825e6e5d087be",
        "C-RAW 0x200 adaptive QP, odd QP-map height, largest sensor (~48 MP)",
    ),
    (
        "cr3-eos-r5-canon-eos-r5-craw-iso-100-nocrop-dual.cr3",
        "00b80633af35c0f85b0299a1d2ea87c6788a9007a1df8a3c5c05ea487b2549e1",
        "C-RAW 0x200 Dual Pixel",
    ),
];

/// Nightly-gate baseline: median MP/s per file, recorded 2026-10-09 on the baseline machine
/// (Windows, 12 cores, rustc 1.99.0 x86_64-pc-windows-gnu, `--release`, best per-file run
/// median across quiet 9-pass runs at High process priority — see `docs/cr3-perf-baseline.md`
/// for the full table and methodology). Keep in sync with that file: the gate compares
/// same-machine, same-conditions only, and a machine under load can measure up to 2× slower —
/// the nightly runner must run quiet (see the doc). Order matches [`CASES`].
const BASELINE_MP_S: &[f64] = &[70.11, 42.01, 69.47, 39.42, 40.48];

/// The nightly gate fails below 75% of baseline (25% tolerance for machine-load jitter).
const GATE_TOLERANCE: f64 = 0.75;

/// Catastrophe floor for the default tier: per-file MP/s that any healthy decode clears by a
/// wide margin but a 10× entropy-style regression does not — one tenth of the recorded
/// baseline, with an absolute backstop so a foreign (much slower) CI machine still passes.
/// The dev profile compiles this crate at opt-level 1, so it gets half the release floor
/// (measured ~75–85% of release speed here). These exist so the assertion can never flake in
/// CI, not to police performance; the nightly gate below is the precise guard.
fn floor_mp_s(baseline: f64) -> f64 {
    let (backstop, share): (f64, f64) = if cfg!(debug_assertions) { (0.25, 0.05) } else { (1.0, 0.1) };
    backstop.max(baseline * share)
}

/// Timed passes per file. One warm-up pass always precedes them. Override with `LC_PERF_ITERS`
/// (the nightly runner may raise it); dev builds time a single pass to keep unattended runs quick.
fn iterations() -> usize {
    if cfg!(debug_assertions) {
        return 1;
    }
    std::env::var("LC_PERF_ITERS").ok().and_then(|v| v.parse().ok()).filter(|&n| n > 0).unwrap_or(3)
}

fn median(values: &mut [f64]) -> f64 {
    values.sort_by(f64::total_cmp);
    let n = values.len();
    if n.is_multiple_of(2) { (values[n / 2 - 1] + values[n / 2]) / 2.0 } else { values[n / 2] }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn cr3_decode_perf_floor_and_optional_nightly_gate() {
    let gating = std::env::var_os("LC_PERF_GATE").is_some_and(|v| v == "1");
    const { assert!(BASELINE_MP_S.len() == CASES.len(), "baseline table out of sync with CASES") };
    if gating && cfg!(debug_assertions) {
        panic!("LC_PERF_GATE=1 requires --release: dev-profile numbers would just fail the gate on their own");
    }
    let iters = iterations();
    let dir = corpus_root().join("raw");
    let mut measured = 0;
    eprintln!(
        "CR3 decode performance ({} files, {iters} timed pass(es) after 1 warm-up{})",
        CASES.len(),
        if gating { ", NIGHTLY GATE vs committed baseline, 25% tolerance — not for PRs" } else { "" }
    );
    for (i, (name, sha, what)) in CASES.iter().enumerate() {
        let path = dir.join(name);
        let Ok(bytes) = std::fs::read(&path) else {
            assert!(!gating, "LC_PERF_GATE=1 needs the full corpus: {} absent", path.display());
            eprintln!("skip: {} absent", path.display());
            continue;
        };
        let actual_sha = sha256_hex(&bytes);
        if actual_sha != *sha {
            assert!(
                !gating,
                "{name}: corpus file differs from its pinned xtask SHA-256 — re-fetch with `cargo xtask corpus --download` before gating"
            );
            eprintln!("warn: {name} does not match its pinned SHA-256; numbers are not baseline-comparable");
        }
        // Warm-up pass: first decode pays page-cache, allocator and lazy static costs.
        let warm = lightcraft_raw::decode(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
        let (w, h) = (warm.width, warm.height);
        let mp = (w * h) as f64 / 1e6;
        drop(warm);
        let mut samples_ms = Vec::with_capacity(iters);
        for _ in 0..iters {
            let t0 = Instant::now();
            let img = lightcraft_raw::decode(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
            std::hint::black_box(&img);
            drop(img);
            samples_ms.push(t0.elapsed().as_secs_f64() * 1e3);
        }
        let med_ms = median(&mut samples_ms);
        let min_ms = samples_ms.iter().copied().reduce(f64::min).unwrap_or(f64::INFINITY);
        let mp_s = mp / (med_ms / 1e3);
        // Printed as measured, so a failed assertion below still leaves every number visible.
        eprintln!("{name:55} {w:>5}x{h:<5} {mp:6.1} MP  median {med_ms:9.1} ms  min {min_ms:9.1} ms  {mp_s:8.2} MP/s  ({what})");
        let floor = floor_mp_s(BASELINE_MP_S[i]);
        assert!(mp_s >= floor, "{name}: {mp_s:.2} MP/s is below the {floor:.2} MP/s catastrophe floor — a decode path regressed badly");
        if gating {
            let baseline = BASELINE_MP_S[i];
            let gate_floor = baseline * GATE_TOLERANCE;
            assert!(
                mp_s >= gate_floor,
                "{name}: {:.2} MP/s is more than 25% below the committed baseline ({baseline:.2} MP/s, i.e. below the {gate_floor:.2} MP/s gate floor). \
                 If the baseline machine or methodology changed, re-record docs/cr3-perf-baseline.md first; otherwise treat this as a performance regression.",
                mp_s
            );
        }
        measured += 1;
    }
    eprintln!(
        "{measured} of {} files measured. Full corpus table + methodology: docs/cr3-perf-baseline.md. Numbers are machine-relative.",
        CASES.len()
    );
    if measured == 0 {
        eprintln!("skip: no corpus files found under {} — run `cargo xtask corpus --download`", dir.display());
    }
}
