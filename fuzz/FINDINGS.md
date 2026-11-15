# Fuzzing findings

Record of every crash/hang the fuzz targets found. A finding here blocks merge
until it is fixed (with a minimized seed promoted into `fuzz/seeds/<target>/`
and a regression test in the owning crate) or explicitly waived.

Status: **no crashes found** so far. The table below is the running log.

| Date | Target | Input | Symptom | Analysis | Status |
|---|---|---|---|---|---|
| 2026-10-09 | all four | seeds + ~30 M mutated inputs (see run log below) | none | — | clean |

## 2026-10-09 initial smoke runs (Windows, `mutate-smoke --release`, seed 20261009, 300 s per target)

| Target | Inputs executed | Throughput | Crashes | Slow cases | Artifacts |
|---|---|---|---|---|---|
| `cr3_container` | 2,011,379 | 6,705/s | 0 | 0 | none |
| `crx_bitstream` | 14,349,293 | 47,831/s | 0 | 0 | none |
| `iso_bmff_boxes` | 105,049,233 | 350,109/s | 0 | 0 | none |
| `cr3_whole_file` | 1,486 (real 18–30 MB EOS R8 Mark II CR3s) | 5/s | 0 | 0 | none |

`cr3_whole_file` ran with the nine real photos as seeds; every seed replays
through probe → probe_info → embedded preview → decode → develop_binned. The
committed 64 KiB header slices also replay clean in phase A of every run.

## Harness-only fixes (not decoder findings)

- The first `wrap_crx_sample` draft put the `CRAW` sample entry beside `stsd`
  instead of inside it, and left the entry's width/height zero — the container
  then reported "CR3 without a Bayer raw track". Caught by the fuzz crate's own
  unit tests (`wrapped_lossless_fixture_decodes`), not by the decoder; fixed
  before the first commit.
- The `crx_bitstream` seed builder initially mis-encoded the v0x200 band-size
  fields (`0/1` instead of `1/2`) and omitted the HF2 zero-run bytes of the
  clamp fixture — caught by `gen-seeds`' self-verification (`probe_info`/`decode`
  assertions against the same fixtures the `crx.rs` unit tests construct),
  fixed before the first commit.

Nothing here reflects a defect in `crates/meta` or `crates/raw`: the hand-written
bounds checks (`get()`-based indexing, box/depth budgets, `checked_*` arithmetic,
`try_reserve_exact` allocation caps) held up under all runs. This matches the
code-review prediction (research/D-code-review.md §3: "Nothing panic-capable was
found reachable from input"), now backed by randomized execution instead of
reading alone.
