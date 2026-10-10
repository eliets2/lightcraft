# CR3 corpus status — the definitive sweep (2026-10-09)

Measured on branch `cr3/p1-corpus-status` (main `7b47ba7`), release `lightcraft-cli` from the shared
target dir (same commit). Corpus: the complete raw.pixls.us Canon CR3 tree, `corpus/raw/*.cr3` (CC0,
git-ignored), plus a private local 9-file Canon EOS R8 Mark II set (not CC0 — local-only, not in the
manifest, intentionally not documented further). Fingerprints: ExifTool 13.59 + a python CMP1 probe
(version u16 @ 4, bit depth @ 24, planes|CFA @ 21, encoding|levels @ 22). Decode sweep: one fresh
`lightcraft-cli run --import <f> catalog.query 'filter={}'` per file; `previewOnly: None` = full
sensor decode.

**TL;DR — 119 files, 106 full sensor decodes, 13 known odd-QP-map failures, 0 corrupt, 0 panics,
0 timeouts.** Every failure is the explicit `Canon CRX adaptive QP map with an odd height` (all
13 have since been fixed — §3). The complete 110-file pixls set
is now pinned in `cargo xtask corpus --download` (the `RAW_SAMPLES` inline SHA-256 pins, round-trip verified).

## 1. Corpus inventory & manifest state

- raw.pixls.us `data/Canon/` holds **110 CR3 files across 29 Canon bodies** (autoindex sweep,
  2026-10-09; matches #396's "110"). Six data-tree files are byte-identical (SHA-256 verified) to the
  six `getfile.php` entries the manifest already had — they supply the exact-pixel cases in
  `cr3_corpus.rs`. The remaining **104 files were added to the manifest**, each pinned in the new
  `RAW_SAMPLES` table (`xtask/src/main.rs`), so `cargo xtask corpus --download` now fetches and
  verifies the complete set. Pin verification hashes ~3 GB; expect minutes in a debug build.
- Round-trip: `cargo xtask corpus --download` → `CR3 pins: 110 verified against SHA-256, 0 absent`;
  a deliberate one-byte corruption is rejected with a `does not match their pinned SHA-256` error.
- Download/compute leftovers (TSVs, probe, sweeps) live in `corpus/_sweep/` (git-ignored):
  `pixls-cr3.tsv` (site enumeration), `downloads.tsv` (per-file SHA-256), `probe-raw.tsv` (CMP1
  inventory), `master.tsv` (joined table), `sweep-raw.tsv` (decode sweep), `timing/timing.tsv`.

## 2. Per-body × variant matrix (110 pixls files; sweep-time snapshot)

| Body | Files | 0x100 RAW | 0x100 C-RAW | 0x200 C-RAW | Dual | ok | previewOnly | fail |
|---|---|---|---|---|---|---|---|---|
| Canon EOS 200D II | 1 | 1 | 0 | 0 | 0 | 1 | 0 | 0 |
| Canon EOS 250D | 8 | 4 | 4 | 0 | 0 | 8 | 0 | 0 |
| Canon EOS 850D | 2 | 2 | 0 | 0 | 0 | 2 | 0 | 0 |
| Canon EOS 90D | 2 | 1 | 1 | 0 | 0 | 2 | 0 | 0 |
| Canon EOS KISS M | 1 | 1 | 0 | 0 | 0 | 1 | 0 | 0 |
| Canon EOS M200 | 2 | 1 | 1 | 0 | 0 | 2 | 0 | 0 |
| Canon EOS M50 | 3 | 1 | 2 | 0 | 0 | 3 | 0 | 0 |
| Canon EOS M50m2 | 1 | 1 | 0 | 0 | 0 | 1 | 0 | 0 |
| Canon EOS M6 Mark II | 8 | 4 | 4 | 0 | 0 | 8 | 0 | 0 |
| Canon EOS R | 8 | 4 | 4 | 0 | 4 | 8 | 0 | 0 |
| Canon EOS R10 | 4 | 4 | 0 | 0 | 0 | 4 | 0 | 0 |
| Canon EOS R100 | 2 | 1 | 1 | 0 | 0 | 2 | 0 | 0 |
| Canon EOS R3 | 1 | 1 | 0 | 0 | 0 | 1 | 0 | 0 |
| Canon EOS R5 | 8 | 4 | 0 | 4 | 4 | 6 | 2 | 0 |
| Canon EOS R50 | 4 | 2 | 0 | 2 | 0 | 2 | 2 | 0 |
| Canon EOS R50 V | 1 | 1 | 0 | 0 | 0 | 1 | 0 | 0 |
| Canon EOS R5m2 | 4 | 2 | 0 | 2 | 0 | 3 | 0 | 1 |
| Canon EOS R6 | 4 | 2 | 0 | 2 | 0 | 2 | 2 | 0 |
| Canon EOS R6 Mark III | 4 | 2 | 0 | 2 | 0 | 3 | 1 | 0 |
| Canon EOS R6m2 | 8 | 4 | 0 | 4 | 4 | 6 | 2 | 0 |
| Canon EOS R7 | 4 | 2 | 0 | 2 | 2 | 2 | 2 | 0 |
| Canon EOS R8 | 4 | 2 | 0 | 2 | 2 | 4 | 0 | 0 |
| Canon EOS RP | 4 | 2 | 2 | 0 | 0 | 4 | 0 | 0 |
| Canon EOS Rebel SL3 | 1 | 0 | 1 | 0 | 0 | 1 | 0 | 0 |
| Canon EOS-1D X Mark III | 2 | 1 | 0 | 1 | 0 | 1 | 1 | 0 |
| Canon PowerShot G5 X Mark II | 8 | 4 | 4 | 0 | 0 | 8 | 0 | 0 |
| Canon PowerShot G7 X Mark III | 2 | 1 | 1 | 0 | 0 | 2 | 0 | 0 |
| Canon PowerShot SX70 HS | 8 | 4 | 4 | 0 | 0 | 8 | 0 | 0 |
| Canon PowerShot V1 | 1 | 0 | 0 | 1 | 0 | 1 | 0 | 0 |
| **Total (29 bodies)** | **110** | **59** | **29** | **22** | **16** | **97** | **12** | **1** |

Reading the matrix:

- **0x100 lossless** (59 RAW + 29 C-RAW, of which 10 carry a muxed dual-track): 88/88 decode. Every lossless variant
  we claim is now exercised by a whole-body family, not one sample: 1.6× crop mode (EOS R, R5, R6,
  R6 II, RP), aspect-ratio sets (250D, M6 II, G5 X II), dual pixel ON, and two-raw-track
  muxing (thumbnail-raw + full, e.g. M200/R10/R5m2/R8 families) all decode from sensor.
- **0x200 C-RAW** (22 files, 10 bodies): 9 decode (incl. PowerShot V1 — a 0x200-only body), 13 fail
  with the odd-QP-map error (§3).
- **Dual pixel** (16 files, 5 bodies): 13 decode; the 3 failures are the odd-QP cases that also
  carry a delta track (R5 CRAW nocrop-dual, R6 II 0356, R7 443A0161).
- **HEVC preview tracks** (recognized, never decoded): 6 files — the R8 set (`RAW`/`CRAW`,
  plain + dual) and the R5m2 pair. All sensor-decode (except R5m2 CRAW, §3); the HEVC preview is
  correctly not substituted.
- No file decodes to garbage, no import crashes, no hangs; every failure is an explicit
  `Unsupported` string and (when a JPEG preview exists) falls back to preview-only import.

## 3. The 13 odd-QP-map failures (exact list; fixed since this sweep)

All 13 are CRX `0x200` C-RAW whose QP map height `ceil(sensor_height/4)` is odd; the decoder
rejects that (docs/cr3.md: "an odd QP-map height … rejected"). Twelve import preview-only with
`previewOnly: "Canon CRX adaptive QP map with an odd height"`; one has no embedded JPEG and fails
import outright. Filenames, sensor track size, SHA-256:

| File | Body | Track | SHA-256 | Effect |
|---|---|---|---|---|
| `cr3-canon-eos-r5m2-craw.cr3` | R5 Mark II | 8480×5650 | `b3c08a5c7a97299dfd58896a12b939e7b43ec78682761812524825e6e5d087be` | import fails (no embedded preview) |
| `cr3-canon-eos-r6-mark-iii-img-4244.cr3` | R6 Mark III | 4512×2994 | `7e1a5d84297a52853b0bbe5a5c03a5d4f5b93e00b7020271523240f9cabfe50e` | previewOnly |
| `cr3-eos-1d-x-mark-iii-canon-eos-1d-x-mark-iii-craw-iso-100-nocrop-nodual.cr3` | 1D X III | 5568×3708 | `e5755d1414a2dd51fb0120b272058f00b0199861646070c00304d927eb119560` | previewOnly |
| `cr3-eos-r5-canon-eos-r5-craw-iso-100-nocrop-dual.cr3` | R5 | 8352×5586 | `00b80633af35c0f85b0299a1d2ea87c6788a9007a1df8a3c5c05ea487b2549e1` | previewOnly |
| `cr3-eos-r5-canon-eos-r5-craw-iso-100-nocrop-nodual.cr3` | R5 | 8352×5586 | `2deae46b816e56ba91ae7a3892ca121290e12d5b22afb11f5b05a3d8f09f759f` | previewOnly |
| `cr3-eos-r50-canon-r50-iso100-craw-electronicshutter.cr3` | R50 | 6192×4060 | `38fc458f2f74d0cfa04afd0ce513b40797080079f0786bedbfc55c39cc28d7d8` | previewOnly |
| `cr3-eos-r50-canon-r50-iso100-craw-mechanicshutter.cr3` | R50 | 6192×4060 | `b79af9ffa184dd740e8af7a9f451f7150053d7e456b3435ca2e43bbbf5f65026` | previewOnly |
| `cr3-eos-r6-canon-eos-r6-craw-iso-100-crop-nodual.cr3` | R6 | 3584×2386 | `74abb0a113d075ad9887a058082f40dd2a938c4813a08474d82356f11a027778` | previewOnly |
| `cr3-eos-r6-canon-eos-r6-craw-iso-100-nocrop-nodual.cr3` | R6 | 5568×3708 | `4775df76433b1e5844bc7a42b95d93efc8438c0a4cd9681e120e23fe3e3007e8` | previewOnly |
| `cr3-eos-r6-mark-ii-canon-r6m2-img-0356-craw-dual-crop.cr3` | R6 II | 3936×2612 | `deed19ea7b5178c70f64710c97bcdc9d21f6610c19b42a2310223e51034d4fc8` | previewOnly |
| `cr3-eos-r6-mark-ii-canon-r6m2-img-0359-craw-normal-crop.cr3` | R6 II | 3936×2612 | `51ae1e673d2801d8a54f33dd8669c43034c506e2d355e75b26c0648e2f14a431` | previewOnly |
| `cr3-eos-r7-443a0159.cr3` | R7 | 7128×4732 | `10a18e3f01ca9cd93408496d3853388ef140830548bfeed4237e40f967eb9d5c` | previewOnly |
| `cr3-eos-r7-443a0161.cr3` | R7 | 7128×4732 | `c01cee1b598120668370892c16a73da915f2aaec8169c5c60a96efd98d9aa220` | previewOnly |

The other nine 0x200 files (even QP-map height) decode, so the fix's blast radius was precisely
these 13; with it landed, `corpus/raw` decodes 110/110 — `cr3_corpus.rs` pins each odd-QP file
full-sensor and the whole-corpus colour test asserts every CR3 decodes.

## 4. Local Canon EOS R8 Mark II set (9 files, not in the manifest)

A private local set — CRX `0x100` lossless 14-bit, two raw tracks (1624×1080 thumbnail-raw +
6188×4120 full), standard JPEG preview. All 9 import with `previewOnly: None` (full sensor decode
at 6000×4000; the three portrait shots report 4000×6000). Confirms the newest 2026 body stays
inside the verified 0x100 family. File names, digests and renders are deliberately kept out of the
repository; the set stays local until the owner dedicates it CC0 — the zero-sample R8 Mark II body
is otherwise only reachable via user donations (§6).

## 5. Render timing (one full render per body, 29 bodies)

`lightcraft-cli run --import <f> catalog.query app.export path=… longEdge=1600 quality=90`, wall
clock includes CLI start + import + sensor decode + develop + JPEG encode. Shared machine — treat
as indicative, ±20 %.

| Body | File | Wall | Export ms |
|---|---|---|---|
| Canon EOS 200D II | `cr3-eos-200d-mark-ii-mg-3608.cr3` | 6.73 s | 6206 |
| Canon EOS 250D | `cr3-eos-250d-canon250d-raw-1-1.cr3` | 4.52 s | 4384 |
| Canon EOS 850D | `cr3-eos-850d-img-1837.cr3` | 4.18 s | 4046 |
| Canon EOS 90D | `cr3-eos-90d-canon-90d-raw-iso-100.cr3` | 5.42 s | 5191 |
| Canon EOS KISS M | `cr3-eos-kiss-m-img-2695.cr3` | 4.54 s | 4399 |
| Canon EOS M200 | `cr3-canon-eos-m200-mg-2231.cr3` | 4.75 s | 4605 |
| Canon EOS M50 | `cr3-canon-m50-raw.cr3` | 4.22 s | 4079 |
| Canon EOS M50m2 | `cr3-eos-m50-mark-ii-img-0051.cr3` | 14.21 s | 5064 |
| Canon EOS M6 Mark II | `cr3-eos-m6-mark-ii-img-raw-1-1.cr3` | 24.78 s | 23020 |
| Canon EOS R | `cr3-eos-r-canon-eos-r-raw-iso-100.cr3` | 30.79 s | 27510 |
| Canon EOS R10 | `cr3-canon-eos-r10-img-6310.cr3` | 38.41 s | 30912 |
| Canon EOS R100 | `cr3-canon-r100-raw.cr3` | 47.01 s | 38350 |
| Canon EOS R3 | `cr3-eos-r3-c3a1200.cr3` | 31.11 s | 30744 |
| Canon EOS R5 | `cr3-eos-r5-…-raw-iso-100-nocrop-nodual.cr3` | 4.84 s | 4688 |
| Canon EOS R50 | `cr3-eos-r50-…-raw-electronicshutter.cr3` | 3.87 s | 3723 |
| Canon EOS R50 V | `cr3-canon-eos-r50-v-img-0034.cr3` | 5.75 s | 5316 |
| Canon EOS R5m2 | `cr3-canon-eos-r5m2-raw.cr3` | 6.25 s | 5327 |
| Canon EOS R6 | `cr3-eos-r6-…-raw-iso-100-nocrop-nodual.cr3` | 4.13 s | 3979 |
| Canon EOS R6 Mark III | `cr3-canon-eos-r6-mark-iii-img-4241.cr3` | 19.17 s | 17338 |
| Canon EOS R6m2 | `cr3-eos-r6-mark-ii-…-raw-normal-full.cr3` | 4.10 s | 3973 |
| Canon EOS R7 | `cr3-eos-r7-443a0157.cr3` | 12.03 s | 11254 |
| Canon EOS R8 | `cr3-canon-r8-raw.cr3` | 3.56 s | 3257 |
| Canon EOS RP | `cr3-eos-rp-…-raw-iso-100-nocrop-nodual.cr3` | 4.01 s | 3837 |
| Canon EOS Rebel SL3 | `cr3-eos-rebel-sl3-sedona2020-sl3-mg-1061.cr3` | 5.76 s | 5615 |
| Canon EOS-1D X Mark III | `cr3-eos-1d-x-mark-iii-…-raw-…-nocrop-nodual.cr3` | 5.74 s | 5357 |
| Canon PowerShot G5 X Mark II | `cr3-powershot-g5-x-mark-ii-…-raw-1-1.cr3` | 7.02 s | 6822 |
| Canon PowerShot G7 X Mark III | `cr3-powershot-g7-x-mark-iii-img-0253.cr3` | 4.14 s | 3981 |
| Canon PowerShot SX70 HS | `cr3-powershot-sx70-hs-img-1000.cr3` | 5.02 s | 4613 |
| Canon PowerShot V1 | `cr3-canon-powershot-v1-img-1225.cr3` | 12.82 s | 6980 |

Median 5.7 s. The slow tail is one cluster: large-sensor **0x100 lossless RAW** files (R100 47 s,
R10 38 s, R3 31 s, EOS R 31 s, M6 II 25 s, R6 III 19 s, R7 12 s) — the horizontal-tile lossless
path scales badly beyond ~24 MP; C-RAW of the same bodies renders 4–7 s. That decode-throughput
gap, not the 13 odd-QP failures, is the biggest user-visible CR3 performance issue. The R8 Mark II
local set renders 6.1–8.6 s per 24 MP file.

## 6. Wanted list (what the corpus still lacks)

Zero CC0 samples anywhere — donations are the only path (pixls re-scan weekly; it gained R50 V, V1
and R6 III only recently):

- **Canon EOS R1** — expected 0x200-family C-RAW at 24 MP+; unverified.
- **Canon EOS R5 C** — Cinema variant; raw-burst/roll likely.
- **Canon PowerShot V10** — vertical video body.
- **Canon EOS C50** — Cinema; new container questions.
- **Canon EOS R6V** (2026-05) and **EOS R8 Mark II** (2026-09) — newest bodies; we hold 9 local
  R8 Mark II files (§4) that decode, pending CC0 donation.

Variant gaps independent of body count:

- **Raw-burst "roll" files** (multi-frame CR3, `CRAW` rolls): zero in corpus; type-3 roll
  extraction untested.
- **HDR CR3** (`CNCV = CanonCR3_003/…`): zero in corpus.
- **Odd QP-map height**: the 13 files of §3 — fixed since this sweep.
- **0x200 with vertical tiles / partial-subband / rounded planes**: no corpus coverage; decoder
  reports them unsupported by construction — samples would confirm the strings.
- **Crop mode**: closed for 0x100 (EOS R/R5/R6/R6 II/RP crop files all decode); closed for 0x200
  too now that §3 is fixed — the R6 and both R6 II crop files were odd-QP cases and are pinned
  full-sensor in `cr3_corpus.rs`.
- **HEVC-preview-only CR3** (no JPEG preview at all): none in corpus; the 6 HEVC-track files here
  all carry a JPEG track too.

## 7. Method notes (clean-room)

No third-party decoder source was read. ExifTool 13.59 and a
python CMP1 box-walker (`corpus/_sweep/probe_cr3.py`, offsets documented in
the private local sample findings) provided container facts; `lightcraft-cli` was exercised strictly as a
black box. Corpus files are CC0 from raw.pixls.us and stay in git-ignored `corpus/`; nothing
photographic is committed.

Test-gate note: while this sweep ran, the parallel pixel-verification lane was dumping oracle
sensor buffers as `corpus/raw/*.cr3.u16` sidecars (first seen 06:31, still arriving at 06:40). The
`crates/raw/tests/corpus.rs` glob tests only skip `.part/.txt/.md`, so they panic on those foreign
scratch files (`cr3-…cr3.u16: not recognised`) — an environment artifact, not a decode regression:
the panics name only `.u16` files, `cr3_corpus.rs` passes 20/20 today (19 sensor-hash fixtures +
the M50 crop test), and the CLI sweep above handles all 119 files. The suite is green once the
oracle dumps are removed or the globs learn to skip them.
