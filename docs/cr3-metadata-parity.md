# CR3 metadata parity vs ExifTool (per body)

What LightCraft records from CR3 metadata, checked against ExifTool 13.59 as the ground truth, per
body and variant across the complete CC0 corpus (`corpus/raw/cr3-*.cr3`: 110 files, 29 bodies).
Companion to `docs/cr3-corpus-status.md` (decode coverage) and `docs/cr3.md`; field priorities
follow `D:\lightcraft-parity\cr3\research\E-corpus-verification.md` §6: (1) as-shot WB,
(2) black/white levels, (3) crop/active area, (4) aspect/crop info, (5) dual pixel, (6) lens,
(7) `CanonModelID`.

## Method (hermetic, no ExifTool at test time)

- **Ground truth:** `exiftool -a -U -G1 -H -v3` over all 110 corpus files, run once; the dumps live
  outside the repository in `D:\lightcraft-parity\cr3\exiftool-dumps\` (ExifTool 13.59,
  `D:\lightcraft-parity\cr3\tools\exiftool.exe`). Only ExifTool's CLI output and its published tag
  documentation were consulted (clean-room: never its source).
- **Extraction side:** LightCraft's own fields via `lightcraft_raw::probe_info` / `decode` — the
  same path the importer uses — dumped as JSON lines by the ignored test
  `cargo test -p lightcraft-raw --test cr3_metadata_parity dump -- --ignored --nocapture`
  (last sweep: `D:\lightcraft-parity\cr3\results\lightcraft-metadata-dump.jsonl`). An import-side
  spot check (`lightcraft-cli run --import <f> catalog.query 'filter={}'`) agrees: camera
  make/model, dimensions and a full decode (`previewOnly: null`) are recorded for every body.
- **Selection:** one base file per body plus the variants that exercise distinct metadata paths
  (crop mode, dual pixel, camera aspect ratios, CRAW, the unparsed ColorData version) = 43 files
  covering all 29 bodies and ColorData versions 16-19, 32-34, 48, 64-66.
- **Harness:** `crates/raw/tests/cr3_metadata_parity.rs` compares LightCraft's fields against a
  committed expected-values table generated from the dumps by
  `python tools/cr3_metadata_reference.py <dump-dir> --update crates/raw/tests/cr3_metadata_parity.rs`.
  WB coefficients must match to 1e-4, black/white levels exactly; the table also *documents known
  absences* (asserted absent, so it records reality). Rows whose corpus file is absent are skipped,
  so the test stays green without the corpus. Regenerate the matrix below with
  `python tools/cr3_metadata_reference.py <dump-dir> --matrix <lightcraft-dump.jsonl>`.

## Field map (ExifTool tag → LightCraft field)

| Ground truth (ExifTool tag) | LightCraft field | Parity |
|---|---|---|
| `Model` (CMT1 IFD0 0x0110) | `metadata.model` | 43/43 exact |
| `WB_RGGBLevelsAsShot` (ColorData 0x4001; word 71/85/105 by version) → green-normalized [R, B] | `wb_multipliers` | 42/43 exact; R6 III absent (gap G1) |
| `PerChannelBlackLevel` (word 0x149/0x157/0x16b/0x17f by version) | `black` 2×2 (order-normalized) | 42/43 exact; R6 III sample-derived (G1) |
| `SpecularWhiteLevel` (word 0x31d/0x32b/0x281/0x295 by version) | `white` scalar (`NormalWhiteLevel` correctly NOT used) | 42/43 exact; R6 III sample-derived (G1) |
| `SensorWidth`/`SensorHeight` (SensorInfo 0x00e0 words 1-2) | full `width`/`height` | 43/43 exact |
| `ExifImageWidth`/`ExifImageHeight` (CMT1 0xa002/0xa003) = recommended crop | `developed_size()` | 42/43 exact; R5m2 APS-C keeps +24×16 px (gap G2) |
| `AspectInfo` `CroppedImageWidth/Height` (CMT3 block) | not applied — full recommended crop developed | documented (gap G3) |
| `LensModel` (ExifIFD 0xa434; the CMT3 0x0095 string agrees on every corpus file) | `metadata.lens_model` | 35/35 interchangeable bodies exact; 8 fixed-lens bodies correctly absent |
| `CanonModelID` (0x4010) | recorded per table row (bookkeeping, not gated) | — |
| Dual Pixel (2nd full-size raw track = delta) | main track wins; sensor dims, WB and levels all match | 4/4 DPR files ok |

Per-version ColorData word offsets in the decoder (`cr3.rs camera_levels`): v16-19 → (71, 329,
797), v32-33 → (85, 343, 811), v34/48 → (105, 363, 641), v64-65 → (105, 383, 661) — each verified
against the dumps' `- Tag 0x…` word offsets (0x47/0x149/0x31d, 0x55/0x157/0x32b, 0x69/0x16b/0x281,
0x69/0x17f/0x295).

## Result

43/43 selected files decode fully; the metadata matches the ground truth everywhere except the
gaps below. **As-shot WB 42/43, black+white levels 42/43, developed size 42/43, lens 43/43,
model 43/43.** All of it is gated by the test.

## Gaps

- **G1 — EOS R6 Mark III: ColorData version 66 unparsed.** `cr3.rs camera_levels` handles versions
  16-19 / 32-33 / 34 / 48 / 64-65 only; v66 falls through to the default, so as-shot WB, black and
  white are absent (levels fall back to sample-derived) — while ExifTool reads them fine. The dump
  shows v66 uses the *same* level offsets as v64/65: `WB_RGGBLevelsAsShot` int16s[4] at word 0x0069
  (= 105; the R6 III file reads `2330 1024 1024 1454`), `PerChannelBlackLevel` int16u[4] at 0x017f
  (= 383; `512 512 512 512`), `SpecularWhiteLevel` int16u[1] at 0x0295 (= 661; 13995;
  `NormalWhiteLevel` at 0x0294 = 13200 is not the clip level). Ingestion is a one-line arm
  (`Some(64 | 65 | 66)`), after which the table row flips from documented-absent to exact.
- **G2 — EOS R5 Mark II APS-C crop: 24×16 px of extra border.** LightCraft develops the IAD1 active
  area (5112×3408); the camera's own recommendation is 5088×3392 (CMT1 `ExifImageWidth/Height`
  0xa002/0xa003 and the `AspectInfo` crop agree). All other corpus files match the recommendation
  exactly. The deviation is pinned in the test's `KNOWN_DEVELOPED_GAPS` so it cannot drift
  silently; fixing it means re-checking the IAD1 vs SensorInfo-border (0x00e0 words 3-6) mapping
  for crop-mode files.
- **G3 — camera `AspectInfo` crop not applied.** Aspect-mode files (1:1, 19:9, 2:3, 4:3, APS-C; 8
  corpus variants) develop the full 3:2 recommended crop. ExifTool exposes the camera's aspect
  crop as `AspectRatio` + `CroppedImageWidth/Height` in the CMT3 AspectInfo block. Documented by
  `ASPECT_APPLIED = false` in the test; the ground truth per file sits in the table's `aspect`
  column. User impact is low (Lightroom also treats the aspect mode as a crop, not a different
  raw), but the developed aspect should eventually follow the camera setting.
- **F1 — nondeterministic CRX decode errors: stale shared-target artifacts, not a code bug (probably).**
  During this work the same verified M200 file (SHA-256 matches the corpus pin) decoded fine in the
  parity test binary but consistently failed in other test binaries with *different* error texts per
  binary (`Rice escape prefix exceeds 41 bits`, `QP coordinate outside map`, `Canon CRX prediction
  outside precision…`), across two commits — and a cold single-file decode reproduced a failure once.
  Cause: the shared `CARGO_TARGET_DIR` is used by several agent worktrees at once (cargo's "file lock
  on build directory" messages), so a test binary can link a `lightcraft_raw` rlib built from
  *another* worktree's in-progress `vendor/crx.rs`; the next workspace does not always rebuild it.
  After forcing a rebuild from this branch's source (`touch crates/raw/src/lib.rs && cargo test
  -p lightcraft-raw`) everything passes: the full suite including `corpus_raw_decodes` (11/11) and
  13+ parity sweeps. Two cheap guards keep this out of the metadata gate: the harness retries a
  failed decode once and asserts both attempts agree. When a CR3 decode "fails", rebuild before
  diagnosing.

## Per-body parity matrix (43 files, 29 bodies)

`(gt / lc)` = ground truth / LightCraft; "known gap" marks a deviation the committed table
documents (G1-G3), not an unnoticed mismatch.

| Body | File (variant) | ColorData | Decode | As-shot WB (gt / lc) | Black 4ch (gt / lc) | White (gt / lc) | Developed (gt / lc) | Lens |
|---|---|---|---|---|---|---|---|---|
| Canon EOS M200 | `cr3-canon-eos-m200-mg-2231.cr3` | 19 | ok | R 1.5703 B 1.2236 / R 1.5703 B 1.2236 (match) | 511/510/511/511 / 510/511/511/511 (match) | 11606 / 11606 (match) | 6000x4000 / 6000x4000 (match) | match |
| Canon EOS R10 | `cr3-canon-eos-r10-img-6310.cr3` | 48 | ok | R 1.8584 B 1.5752 / R 1.8584 B 1.5752 (match) | 2048/2048/2048/2048 / 2048/2048/2048/2048 (match) | 14338 / 14338 (match) | 6000x4000 / 6000x4000 (match) | match |
| Canon EOS R50 V | `cr3-canon-eos-r50-v-img-0034.cr3` | 65 | ok | R 1.7900 B 1.6436 / R 1.7900 B 1.6436 (match) | 512/512/512/512 / 512/512/512/512 (match) | 11708 / 11708 (match) | 6000x4000 / 6000x4000 (match) | match |
| Canon EOS R5m2 | `cr3-canon-eos-r5m2-aps-c-raw.cr3` | 64 | ok | R 1.7383 B 1.9248 / R 1.7383 B 1.9248 (match) | 512/512/512/512 / 512/512/512/512 (match) | 14888 / 14888 (match) | 5088x3392 / 5112x3408 (known gap) | match |
| Canon EOS R5m2 | `cr3-canon-eos-r5m2-raw.cr3` | 64 | ok | R 1.8350 B 1.7627 / R 1.8350 B 1.7627 (match) | 512/512/512/512 / 512/512/512/512 (match) | 14888 / 14888 (match) | 8192x5464 / 8192x5464 (match) | match |
| Canon EOS R6 Mark III | `cr3-canon-eos-r6-mark-iii-img-4241.cr3` | 66 | ok | absent (ColorData v66 unparsed) / absent (ColorData v66 unparsed) (documented gap) | sample-derived / sample-derived (n/a) | sample-derived / sample-derived (n/a) | 6960x4640 / 6960x4640 (match) | match |
| Canon EOS M50 | `cr3-canon-m50-raw.cr3` | 16 | ok | R 1.0771 B 1.8994 / R 1.0771 B 1.8994 (match) | 510/511/511/511 / 510/511/511/511 (match) | 11892 / 11892 (match) | 6000x4000 / 6000x4000 (match) | match |
| Canon PowerShot V1 | `cr3-canon-powershot-v1-img-1225.cr3` | 65 | ok | R 1.8896 B 1.4990 / R 1.8896 B 1.4990 (match) | 512/512/512/512 / 512/512/512/512 (match) | 13036 / 13036 (match) | 5760x3840 / 5760x3840 (match) | match |
| Canon EOS R100 | `cr3-canon-r100-raw.cr3` | 19 | ok | R 1.7930 B 1.7441 / R 1.7930 B 1.7441 (match) | 2048/2048/2048/2048 / 2048/2048/2048/2048 (match) | 10439 / 10439 (match) | 6000x4000 / 6000x4000 (match) | match |
| Canon EOS R8 | `cr3-canon-r8-raw.cr3` | 48 | ok | R 1.8516 B 1.5879 / R 1.8516 B 1.5879 (match) | 512/512/512/512 / 512/512/512/512 (match) | 14008 / 14008 (match) | 6000x4000 / 6000x4000 (match) | match |
| Canon EOS-1D X Mark III | `cr3-eos-1d-x-mark-iii-canon-eos-1d-x-mark-iii-raw-iso-100-nocrop-nodual.cr3` | 32 | ok | R 1.5977 B 1.7773 / R 1.5977 B 1.7773 (match) | 512/512/512/512 / 512/512/512/512 (match) | 14008 / 14008 (match) | 5472x3648 / 5472x3648 (match) | match |
| Canon EOS 200D II | `cr3-eos-200d-mark-ii-mg-3608.cr3` | 18 | ok | R 1.9502 B 1.5352 / R 1.9502 B 1.5352 (match) | 2049/2049/2049/2049 / 2049/2049/2049/2049 (match) | 14338 / 14338 (match) | 6000x4000 / 6000x4000 (match) | match |
| Canon EOS 250D | `cr3-eos-250d-canon250d-craw-3-2.cr3` | 18 | ok | R 1.8418 B 1.6357 / R 1.8418 B 1.6357 (match) | 511/511/511/511 / 511/511/511/511 (match) | 11606 / 11606 (match) | 6000x4000 / 6000x4000 (match) | match |
| Canon EOS 250D | `cr3-eos-250d-canon250d-raw-1-1.cr3` | 18 | ok | R 1.8418 B 1.6357 / R 1.8418 B 1.6357 (match) | 511/511/511/511 / 511/511/511/511 (match) | 11606 / 11606 (match) | 6000x4000 / 6000x4000 (match) | match |
| Canon EOS 250D | `cr3-eos-250d-canon250d-raw-19-9.cr3` | 18 | ok | R 1.8418 B 1.6357 / R 1.8418 B 1.6357 (match) | 511/511/511/511 / 511/511/511/511 (match) | 11606 / 11606 (match) | 6000x4000 / 6000x4000 (match) | match |
| Canon EOS 250D | `cr3-eos-250d-canon250d-raw-2-3.cr3` | 18 | ok | R 1.8418 B 1.6357 / R 1.8418 B 1.6357 (match) | 511/511/511/511 / 511/511/511/511 (match) | 11606 / 11606 (match) | 6000x4000 / 6000x4000 (match) | match |
| Canon EOS 850D | `cr3-eos-850d-img-1837.cr3` | 19 | ok | R 2.0518 B 1.4951 / R 2.0518 B 1.4951 (match) | 511/511/511/511 / 511/511/511/511 (match) | 11606 / 11606 (match) | 6000x4000 / 6000x4000 (match) | match |
| Canon EOS 90D | `cr3-eos-90d-canon-90d-raw-iso-100.cr3` | 19 | ok | R 2.0479 B 1.3438 / R 2.0479 B 1.3438 (match) | 512/512/512/512 / 512/512/512/512 (match) | 13036 / 13036 (match) | 6960x4640 / 6960x4640 (match) | match |
| Canon EOS KISS M | `cr3-eos-kiss-m-img-2695.cr3` | 16 | ok | R 1.5830 B 1.3252 / R 1.5830 B 1.3252 (match) | 510/510/510/509 / 509/510/510/510 (match) | 11892 / 11892 (match) | 6000x4000 / 6000x4000 (match) | match |
| Canon EOS M50m2 | `cr3-eos-m50-mark-ii-img-0051.cr3` | 19 | ok | R 1.3027 B 1.5215 / R 1.3027 B 1.5215 (match) | 2049/2049/2049/2049 / 2049/2049/2049/2049 (match) | 10439 / 10439 (match) | 6000x4000 / 6000x4000 (match) | match |
| Canon EOS M6 Mark II | `cr3-eos-m6-mark-ii-img-raw-1-1.cr3` | 19 | ok | R 1.5732 B 2.0078 / R 1.5732 B 2.0078 (match) | 2048/2048/2048/2048 / 2048/2048/2048/2048 (match) | 13660 / 13660 (match) | 6960x4640 / 6960x4640 (match) | match |
| Canon EOS M6 Mark II | `cr3-eos-m6-mark-ii-img-raw-3-2.cr3` | 19 | ok | R 1.5684 B 2.0117 / R 1.5684 B 2.0117 (match) | 2048/2048/2048/2048 / 2048/2048/2048/2048 (match) | 10852 / 10852 (match) | 6960x4640 / 6960x4640 (match) | match |
| Canon EOS R | `cr3-eos-r-canon-eos-r-raw-iso-100.cr3` | 17 | ok | R 1.9541 B 1.5449 / R 1.9541 B 1.5449 (match) | 511/510/511/510 / 510/510/511/511 (match) | 14008 / 14008 (match) | 6720x4480 / 6720x4480 (match) | match |
| Canon EOS R | `cr3-eos-r-canon-eos-r-raw-iso-100-crop-nodual.cr3` | 17 | ok | R 1.9727 B 1.5469 / R 1.9727 B 1.5469 (match) | 511/510/511/510 / 510/510/511/511 (match) | 14008 / 14008 (match) | 4176x2784 / 4176x2784 (match) | match |
| Canon EOS R | `cr3-eos-r-canon-eos-r-raw-iso-100-dual-pixel-on.cr3` DPR | 17 | ok | R 1.9658 B 1.5332 / R 1.9658 B 1.5332 (match) | 510/510/510/509 / 509/510/510/510 (match) | 14008 / 14008 (match) | 6720x4480 / 6720x4480 (match) | match |
| Canon EOS R3 | `cr3-eos-r3-c3a1200.cr3` | 34 | ok | R 2.0234 B 1.7012 / R 2.0234 B 1.7012 (match) | 2047/2047/2047/2047 / 2047/2047/2047/2047 (match) | 14351 / 14351 (match) | 6000x4000 / 6000x4000 (match) | match |
| Canon EOS R5 | `cr3-eos-r5-canon-eos-r5-raw-iso-100-crop-nodual.cr3` | 33 | ok | R 1.5400 B 2.4443 / R 1.5400 B 2.4443 (match) | 510/510/510/510 / 510/510/510/510 (match) | 14008 / 14008 (match) | 5088x3392 / 5088x3392 (match) | match |
| Canon EOS R5 | `cr3-eos-r5-canon-eos-r5-raw-iso-100-nocrop-dual.cr3` DPR | 33 | ok | R 1.7178 B 2.1377 / R 1.7178 B 2.1377 (match) | 512/512/512/512 / 512/512/512/512 (match) | 14008 / 14008 (match) | 8192x5464 / 8192x5464 (match) | match |
| Canon EOS R5 | `cr3-eos-r5-canon-eos-r5-raw-iso-100-nocrop-nodual.cr3` | 33 | ok | R 1.6201 B 2.2607 / R 1.6201 B 2.2607 (match) | 510/510/510/510 / 510/510/510/510 (match) | 14008 / 14008 (match) | 8192x5464 / 8192x5464 (match) | match |
| Canon EOS R50 | `cr3-eos-r50-canon-r50-iso100-raw-electronicshutter.cr3` | 48 | ok | R 1.7266 B 1.6953 / R 1.7266 B 1.6953 (match) | 512/512/512/512 / 512/512/512/512 (match) | 11606 / 11606 (match) | 6000x4000 / 6000x4000 (match) | match |
| Canon EOS R6 | `cr3-eos-r6-canon-eos-r6-raw-iso-100-crop-nodual.cr3` | 33 | ok | R 1.8125 B 1.6309 / R 1.8125 B 1.6309 (match) | 512/512/512/512 / 512/512/512/512 (match) | 14008 / 14008 (match) | 3408x2272 / 3408x2272 (match) | match |
| Canon EOS R6 | `cr3-eos-r6-canon-eos-r6-raw-iso-100-nocrop-nodual.cr3` | 33 | ok | R 1.8057 B 1.6387 / R 1.8057 B 1.6387 (match) | 512/512/512/512 / 512/512/512/512 (match) | 14008 / 14008 (match) | 5472x3648 / 5472x3648 (match) | match |
| Canon EOS R6m2 | `cr3-eos-r6-mark-ii-canon-r6m2-img-0346-raw-normal-full.cr3` | 48 | ok | R 2.1465 B 1.2930 / R 2.1465 B 1.2930 (match) | 512/512/512/512 / 512/512/512/512 (match) | 14008 / 14008 (match) | 6000x4000 / 6000x4000 (match) | match |
| Canon EOS R6m2 | `cr3-eos-r6-mark-ii-canon-r6m2-img-0349-raw-dual-crop.cr3` DPR | 48 | ok | R 2.0938 B 1.3301 / R 2.0938 B 1.3301 (match) | 512/512/512/512 / 512/512/512/512 (match) | 14008 / 14008 (match) | 3744x2496 / 3744x2496 (match) | match |
| Canon EOS R7 | `cr3-eos-r7-443a0157.cr3` | 48 | ok | R 1.7041 B 1.6592 / R 1.7041 B 1.6592 (match) | 512/511/511/512 / 511/511/512/512 (match) | 13036 / 13036 (match) | 6960x4640 / 6960x4640 (match) | match |
| Canon EOS R8 | `cr3-eos-r8-raw-full-frame-dual-pixel.cr3` DPR | 48 | ok | R 1.8584 B 1.5928 / R 1.8584 B 1.5928 (match) | 512/512/512/512 / 512/512/512/512 (match) | 14008 / 14008 (match) | 6000x4000 / 6000x4000 (match) | match |
| Canon EOS Rebel SL3 | `cr3-eos-rebel-sl3-sedona2020-sl3-mg-1061.cr3` | 18 | ok | R 1.8418 B 1.6025 / R 1.8418 B 1.6025 (match) | 511/511/511/511 / 511/511/511/511 (match) | 11606 / 11606 (match) | 6000x4000 / 6000x4000 (match) | match |
| Canon EOS RP | `cr3-eos-rp-canon-eos-rp-raw-iso-100-crop-nodual.cr3` | 18 | ok | R 1.7295 B 2.1152 / R 1.7295 B 2.1152 (match) | 513/513/513/513 / 513/513/513/513 (match) | 14558 / 14558 (match) | 3888x2592 / 3888x2592 (match) | match |
| Canon EOS RP | `cr3-eos-rp-canon-eos-rp-raw-iso-100-nocrop-nodual.cr3` | 18 | ok | R 1.7471 B 2.0479 / R 1.7471 B 2.0479 (match) | 513/513/513/513 / 513/513/513/513 (match) | 14558 / 14558 (match) | 6240x4160 / 6240x4160 (match) | match |
| Canon PowerShot G5 X Mark II | `cr3-powershot-g5-x-mark-ii-canon-powershot-g5-x-mark-ii-raw-1-1.cr3` | 19 | ok | R 2.5156 B 1.7266 / R 2.5156 B 1.7266 (match) | 2048/2048/2048/2048 / 2048/2048/2048/2048 (match) | 14335 / 14335 (match) | 5472x3648 / 5472x3648 (match) | match |
| Canon PowerShot G5 X Mark II | `cr3-powershot-g5-x-mark-ii-canon-powershot-g5-x-mark-ii-raw-3-2.cr3` | 19 | ok | R 2.5098 B 1.6621 / R 2.5098 B 1.6621 (match) | 2048/2048/2048/2048 / 2048/2048/2048/2048 (match) | 14335 / 14335 (match) | 5472x3648 / 5472x3648 (match) | match |
| Canon PowerShot G7 X Mark III | `cr3-powershot-g7-x-mark-iii-img-0253.cr3` | 19 | ok | R 2.2510 B 1.8652 / R 2.2510 B 1.8652 (match) | 2048/2048/2048/2048 / 2048/2048/2048/2048 (match) | 14335 / 14335 (match) | 5472x3648 / 5472x3648 (match) | match |
| Canon PowerShot SX70 HS | `cr3-powershot-sx70-hs-img-1000.cr3` | 18 | ok | R 1.5469 B 1.7627 / R 1.5469 B 1.7627 (match) | 512/512/512/512 / 512/512/512/512 (match) | 12500 / 12500 (match) | 5184x3888 / 5184x3888 (match) | match |

Black 4-channel values are compared order-normalized (the 2×2 grid's CFA phase may differ between
the camera's recording order and LightCraft's; the values agree exactly).

## Recommended ingestion order

1. **G1 (R6 III ColorData v66 levels)** — smallest change, biggest coverage: one version arm in
   `cr3.rs camera_levels`; the ground truth is already committed in the table row and will gate it
   the moment it lands (the row's documented-absent asserts flip to exact matches).
2. **G2 (R5m2 APS-C crop trim)** — revisit the IAD1 default crop for crop-mode files against the
   camera's 0xa002/0xa003 recommendation (24×16 px on the only deviating file).
3. **G3 (AspectInfo crop)** — largest scope (new CMT3 block + developed-size plumbing); do it
   together with the crop UI so the camera aspect can also be overridden.
4. **F1 (shared `CARGO_TARGET_DIR` hazard)** — not a code change: anyone diagnosing a "failing"
   CR3 decode on a shared target dir should force a rebuild from their own worktree first
   (see F1); a `cargo clean -p lightcraft-raw` between agent worktrees would remove the hazard.

Regeneration: dumps in `D:\lightcraft-parity\cr3\exiftool-dumps\`; table via
`python tools/cr3_metadata_reference.py <dump-dir> --update crates/raw/tests/cr3_metadata_parity.rs`;
matrix via `--matrix <lightcraft-dump.jsonl>` (see Method).
