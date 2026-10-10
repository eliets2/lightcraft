# Fuzzing LightCraft's CR3/CRX parsers

Four `cargo-fuzz` targets guard the never-panic contract of the CR3 container
(`crates/meta/src/cr3.rs`) and the CRX bitstream decoder
(`crates/raw/src/vendor/{cr3,crx,crx_wavelet}.rs`). Precedent: DNGLab 0.8.0
replaced AFL with cargo-fuzz for exactly this parser family. Design and guard
values follow the verification plan (research/E-corpus-verification.md §3.1).

| Target | Body | Seeds in `seeds/` |
|---|---|---|
| `cr3_container` | `parse_cr3` + `Cr3Compression`/`Cr3ImageArea::parse` + `merged_exif`/`timed_exif` + `probe`/`probe_info` (CRX header walk) + `embedded_preview`, on raw bytes | synthetic containers + first 64 KiB of real files |
| `crx_bitstream` | structure-aware: first input byte picks a **capped** CMP1 descriptor (≤ 64×48, so one decode is milliseconds at worst), the rest replaces the CRX sample payload inside a valid container; runs `probe_info` **and** full `decode` | the synthetic CRX sample streams from the `crx.rs` unit tests (lossless, C-RAW, v0x200 adaptive QP, clamp overshoot) |
| `cr3_whole_file` | byte-mutated real CR3s through probe → probe_info → preview extraction → decode → develop (binned preview path; full develop ≤ 24 MP) | 64 KiB headers (committed); full files stay uncommitted (see `seeds/README.md`) |
| `iso_bmff_boxes` | structure-aware `arbitrary` generator over the known fourcc pool (`ftyp moov uuid trak mdia minf stbl stsd CRAW CMP1 CDI1 IAD1 CTMD CMT1–4 THMB CTBO CCTP CNCV PRVW mdat …`) with consistent sizes plus occasional size–content lies; output runs through the same sink as `cr3_container` | two synthetic box graphs (consistent / size-lie) |

## Layout

```
fuzz/
├── Cargo.toml            fuzz crate; `libfuzzer` feature OFF by default (see below)
├── fuzz_targets/*.rs     four bins: a plain #[no_mangle] pub extern "C"
│                         LLVMFuzzerTestOneInput shim (libFuzzer build) or a
│                         stub main that explains how to run (stable build)
├── src/lib.rs            the fuzzed bodies + synthetic container/box builders
├── src/bin/gen-seeds.rs  regenerates seeds/ (self-verifying, deterministic)
├── src/bin/mutate-smoke.rs  deterministic std-only mutation fuzzer (any platform)
├── seeds/<target>/       committed regression corpus (see seeds/README.md)
├── FINDINGS.md           crashes found: minimized input + stack + analysis
└── README.md             this file
```

The actual code under test lives in the target bodies in `src/lib.rs`; the
libFuzzer shims and `mutate-smoke` call the *same* functions, so the Windows
smoke harness and Linux CI exercise byte-for-byte identical code.

## Why the `libfuzzer` feature is off by default

cargo-fuzz/libFuzzer does not run on windows-gnu (and needs nightly + a Unix
C++ toolchain). With `default = []`, every normal build (`cargo
build/test/clippy --workspace`, any platform, stable toolchain) compiles only
the stub binaries — no C++ runtime, no nightly. Linux CI enables the feature
explicitly; `cargo fuzz run` forwards it (verified in cargo-fuzz `options.rs`:
`--features` / `--no-default-features` are passed through to cargo build).

The fuzz crate deliberately does **not** opt into the workspace lints: the
`LLVMFuzzerTestOneInput` shims contain one small `unsafe` block each (the
libFuzzer ABI: a raw pointer + length), which is the only unsafe code outside
`crates/sysmem`, and the smoke harness is allowed `ExitCode`-style error exits.
The fuzzed bodies in `src/lib.rs` are panic-free and `#![forbid(unsafe_code)]`;
anything that panics there is a finding by definition.

## Running locally on Windows (today)

```bash
export CARGO_TARGET_DIR="$(pwd)/../target"   # shared target dir
# run from your LightCraft worktree/checkout root

# build once (stable toolchain is enough)
cargo build --release -p lightcraft-fuzz

# ~5 min deterministic mutation smoke per target (default is 5 minutes)
cargo run --release -p lightcraft-fuzz --bin mutate-smoke -- \
    cr3_container  fuzz/seeds/cr3_container  --minutes 5 --seed 20261009
cargo run --release -p lightcraft-fuzz --bin mutate-smoke -- \
    crx_bitstream  fuzz/seeds/crx_bitstream  --minutes 5 --seed 20261009
cargo run --release -p lightcraft-fuzz --bin mutate-smoke -- \
    iso_bmff_boxes fuzz/seeds/iso_bmff_boxes --minutes 5 --seed 20261009

# whole-file fuzzing needs real CR3s (never committed): pass their directory
cargo run --release -p lightcraft-fuzz --bin mutate-smoke -- \
    cr3_whole_file fuzz/seeds/cr3_whole_file \
    "D:\path\to\private\CR3s" --minutes 5 --seed 20261009
```

The harness replays every committed seed unmodified first (a panic there blocks
merge by itself), then loops deterministic mutations: bit flips, byte
substitutions, interesting-byte runs, truncations, insertions, donor splices,
**box-size lies** and chunk duplication. It exits 1 on any panic and saves the
failing input under `fuzz/artifacts-smoke/<target>/`. A finding prints the
`--seed` and case number: re-running with the same seed replays the exact
sequence. Cases slower than 5 s are reported as slow findings (in-process
harnesses cannot kill a hang — treat a slow finding as a potential infinite
loop and investigate).

Budget: the fast targets run at ~10⁴–10⁶ exec/s (release build is required);
`cr3_whole_file` runs at a few exec/s because each seed is a real 20–30 MB
photo — leave it minutes, not seconds.

## Running for real (Linux CI — nightly toolchain)

```bash
rustup toolchain install nightly --profile minimal
cargo install cargo-fuzz

cd fuzz

# PR smoke: 5 minutes per target (cargo-fuzz book recipe, pinned action style)
cargo fuzz run cr3_container  --features libfuzzer -- -max_len=1048576 -rss_limit_mb=2048 -malloc_limit_mb=2048 -timeout=25 -print_final_stats=1 -max_total_time=300
cargo fuzz run crx_bitstream  --features libfuzzer -- -max_len=8388608 -rss_limit_mb=2048 -malloc_limit_mb=2048 -timeout=25 -print_final_stats=1 -max_total_time=300
cargo fuzz run iso_bmff_boxes --features libfuzzer -- -max_len=1048576 -rss_limit_mb=2048 -malloc_limit_mb=2048 -timeout=25 -print_final_stats=1 -max_total_time=300
cargo fuzz run cr3_whole_file --features libfuzzer -- -max_len=0 -rss_limit_mb=2048 -malloc_limit_mb=2048 -timeout=25 -print_final_stats=1 -max_total_time=300
```

Guard values (plan E §3.1): `-max_len` 1 MiB for container targets / 8 MiB for
the bitstream target / 0 (= longest seed) for whole files; `-rss_limit_mb=2048`
and `-malloc_limit_mb=2048` cap allocation bombs; `-timeout=25` kills a hung
case; `-print_final_stats=1` for the run log.

- **Regression mode (merge gate):** the commands above without
  `-max_total_time` replay `fuzz/seeds/<target>/` unconditionally and exit 0
  only if no seed crashes — this is the check every PR must pass before the
  time-boxed exploration. A reproducible crash on any target blocks merge; the
  fix lands together with the minimized seed (below).
- **Nightly:** the same four commands with `-max_total_time=3600` (60 min each)
  on a scheduled runner; upload `fuzz/artifacts/` on failure. Add
  `--sanitizer address` (default) and, for the nightly job,
  `--sanitizer memory` for a second pass over the seed corpus.
- **Weekly:** `cargo fuzz cmin <target>` per target to keep the live corpus
  lean; merge any newly minimized crash artifacts into `seeds/`.

GitHub Actions sketch (pinned cargo-fuzz version per the cargo-fuzz book):

```yaml
jobs:
  fuzz-smoke:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@nightly
      - run: cargo install --version <PINNED> cargo-fuzz
      - run: cargo xtask corpus --download          # CC0 corpus for cr3_whole_file
      - run: |
          cd fuzz
          for t in cr3_container crx_bitstream iso_bmff_boxes cr3_whole_file; do
            cargo fuzz run $t --features libfuzzer -- \
              -rss_limit_mb=2048 -malloc_limit_mb=2048 -timeout=25 \
              -print_final_stats=1 -max_total_time=300
          done
      - uses: actions/upload-artifact@v4
        if: failure()
        with: { name: crash-artifacts, path: fuzz/artifacts }
```

## Crash → minimized seed → merge gate

1. Reproduce: `cargo fuzz run <target> --features libfuzzer fuzz/artifacts/<crash>`
   (or locally: `mutate-smoke <target> <artifact> --minutes 0 --ops 1`).
2. Minimize: `cargo fuzz tmin <target> <artifact>` (shrink to the smallest
   failing input; libFuzzer's `-minimize_crash=1` works too).
3. Commit the minimized input as a seed in `fuzz/seeds/<target>/`, add the
   analysis to `FINDINGS.md`, and fix the crash with a synthetic regression
   test that panicked before the fix (crashes in `crates/` are reported to the
   crate owners — the fuzz suite fixes none of them directly).
4. The regression-mode CI run replays all seeds, so the minimized input is now
   a permanent merge gate.
