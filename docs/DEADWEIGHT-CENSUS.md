# Dead-weight census (campaign wave 1)

Measured on opt/perf-campaign @ 0.14.0, Windows, CARGO_TARGET_DIR=D:/_work/lc-opt-target.

## 1. Trial: intra-workspace dependency edge removed (committed)
- `lightcraft-color` no longer depends on `lightcraft-geom`: zero source references, `cargo build -p lightcraft-color --all-targets` clean, 22/22 tests pass. Committed.
- Method proven: per-crate dep-vs-usage grep → removal → crate gates. Roll out to the remaining ~20 crates in wave 2 (each needs its own proof: deps used via macros/tests/build.rs must not be condemned).

## 2. Compiler-proven dead code: NONE
`cargo build --workspace` produces **0** `never used/read/constructed` warnings. Private dead code is not a lever in this repo. The dead weight lives in binary composition and packaging (below), not in unreachable private items.

## 3. Binary composition (the real lever)
- `target/release/lightcraft.exe` = **425.1 MB**; debug = 263.1 MB. Release out-sizes debug because `[profile.release]` sets `debug = "line-tables-only"` — ~160 MB of line tables ride in the exe.
- A slim shipping profile **already exists**: `[profile.dist]` (lto = "thin", codegen-units = 1, `debug = false`). Releases built with `--profile dist` are the honest size baseline; plain `--release` is a developer artifact.
- Levers, in order: (1) confirm the shipped artifact uses `dist`; (2) add `strip = "debuginfo"` + `split-debuginfo` to `[profile.release]` so even dev-release exes shrink (~160 MB); (3) `cargo bloat`/`cargo asm` breakdown of the dist binary to find the next tier (likely raw-decoder tables and the GUI crate pool).
- Not the problem: `assets/` is 4.6 MB (three Inter weights); denoise models are not embedded (runtime download).

## 4. Oversize census (top files, lines)
headless.rs 5 420 · camera_preview.rs 4 550 · export.rs 3 823 · ui-egui panels/detail.rs 3 706 · files.rs 2 935 · menus.rs 2 633. Workspace .rs total ≈ 314 000 lines. Refactor targets for readability, not for size — do not expect binary wins here.

## 5. Safe-removal queue for wave 2
1. Per-crate unused-dependency sweep (trial method, ~20 crates, one proof each).
2. `strip = "debuginfo"` on `[profile.release]` (verify crash-symbolication workflow first — line tables may be intentional for local repro).
3. Measure and publish `dist` vs `release` sizes in docs/releasing.md so the 425 MB number stops being mistaken for the shipping size.
4. `cargo bloat -n 20` on the dist profile to rank in-binary weight (needs one release link ≈ 10-20 min).
