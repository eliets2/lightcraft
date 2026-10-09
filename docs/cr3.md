# Canon CR3 decoding

The CR3 adapter reads the CRX Bayer track, `CMP1` compression descriptor,
`CDI1`/`IAD1` sensor geometry and timed `CTMD` maker notes. White balance,
black/white levels and the default crop come from the file. A Dual Pixel delta
track is never substituted for the primary Bayer track.

The decoder is independent Rust code shared by desktop, CLI and WebAssembly;
it has no operating-system imaging dependency or external decoder. Its sources
are the public prose descriptions and patents cited in
`crates/raw/src/vendor/crx.rs`. Unsupported coding variants retain the existing
embedded-JPEG fallback and its visible warning when a JPEG is present. So does
any other CR3 decode error (a corrupt file, or a body whose CRX stream the
decoder misreads): every CR3 opened from its embedded JPEG before this decoder,
and only three bodies are verified, so a CR3 never fails to import or load
where a preview exists. HEVC-only preview tracks are recognized but are not
decoded.

## Variant status

The authoritative list of supported and rejected CRX coding variants. Every
rejection is an explicit `RawError::Unsupported` whose text is contractual:
`crates/raw/tests/cr3_variants.rs` rebuilds each variant as a synthetic CR3
fixture and asserts the exact string, plus that header probing (`probe_info`,
the import path) and full decoding fail identically — the engine's
preview-only fallback depends on the two layers agreeing on the error class.

| Variant | Status | Gate | Exact rejection |
|---|---|---|---|
| Version `0x100`, 4 planes, encoding 0, CFA 0–3, levels 0 or 3 (horizontal tiles) | Supported | `supports()` `vendor/crx.rs` | — |
| Version `0x200` single-tile 14-bit C-RAW with adaptive QP, even QP-map height, QP 131–167 | Supported | `supports()` + `qp_map` `vendor/crx.rs` | — |
| Version not `0x100`/`0x200`, encoding ≠ 0 inside CRX, planes ≠ 4 inside CRX, CFA > 3, levels ∉ {0, 3} | Rejected | `supports()` `vendor/crx.rs` | `Canon CRX version {version:#x}, encoding {encoding}, {planes} planes, {levels} wavelet levels` |
| Bit depth outside 8–16 (CMP1 parse bounds it to 1–16) | Rejected | `supports()` `vendor/crx.rs` | `Canon CRX {bits}-bit samples` |
| C-RAW with vertical tiles (`tile_height ≠ height`, levels 3) | Rejected | `supports()` `vendor/crx.rs` | `Canon CRX C-RAW with vertical tiles` |
| `0x200` that is not single-tile 14-bit C-RAW | Rejected | `supports()` `vendor/crx.rs` | `Canon CRX version 0x200 requires a single-tile 14-bit Bayer C-RAW image` |
| `0x200` QP map of odd height (13 known files) | Rejected — tracked, fix in flight on another branch | `supports()` + `qp_map` `vendor/crx.rs` | `Canon CRX adaptive QP map with an odd height` |
| Extended CMP1 median-bit-depth header (type-3 burst) | Rejected | `supports()` `vendor/crx.rs` | `Canon CRX extended median-bit-depth header` |
| Rounded-plane coding (plane flags bits 25–26) or wrong plane flags | Rejected | `validate_planes()` `vendor/crx.rs` | `Canon CRX plane coding flags` |
| Partial-subband coding (band flags bit 27) | Rejected | `validate_planes()` `vendor/crx.rs` | `Canon CRX partial subband coding` |
| Adaptive gain on bands 0–3 | Rejected | `validate_planes()` `vendor/crx.rs` | `Canon CRX adaptive quantization subband parameters` |
| Adaptive low-frequency quantization (bands 0–3 at dequantization) | Rejected (unreachable via the public API: the gate above fires first; unit-tested) | `adaptive_step()` `vendor/crx.rs` | `Canon CRX adaptive low-frequency quantization` |
| Uniform quantization value outside 4–43 | Rejected | `quant_step()` via `validate_planes()` `vendor/crx.rs` | `Canon CRX quantization value {value}` |
| Adaptive QP value outside 131–167 | Rejected | `qp_map()` `vendor/crx.rs` | `Canon CRX adaptive QP value {value}` |
| `ff11`/`ff12`/`ff13` marker family inside a `0x100` stream | Rejected | `layout()` `vendor/crx.rs` | `Canon CRX ff11/ff12/ff13 marker family` |
| Marker header length ≠ 8 (`ff01`/`ff02`/`ff03`) or ≠ 16 (`ff11`/`ff12`/`ff13`) | Rejected | `layout()` `vendor/crx.rs` | `Canon CRX marker {marker:#x} header length {size}` |
| Nonzero reserved field or wrong magic in a `0x200` tile header | Rejected | `layout()` `vendor/crx.rs` | `Canon CRX adaptive quantization tile header` |
| Nonzero reserved field in a `0x200` band header | Rejected | `layout()` `vendor/crx.rs` | `Canon CRX adaptive subband reserved field` |
| planes ≠ 4 or encoding ≠ 0 at the container adapter (sRAW/mRAW-era codings) | Rejected | `vendor/cr3.rs` adapter | `Canon CR3 encoding {encoding} with {planes} planes; note: sRAW/mRAW are CR2-era YCbCr codings and do not occur in CR3` |

sRAW/mRAW never occur in CR3: the C-RAW coding replaced them, so those CMP1
encodings in a CR3 container indicate a misdescribed track rather than a
supported Canon format. Descriptor values the `CMP1` parser already bounds —
zero dimensions, a tile larger than the image, bit depth outside 1–16, planes
outside 1–4, or a CFA above 3 on a multi-plane track — never reach these gates:
they make the descriptor unparseable, reported as a corrupt-descriptor error.

## Verified sensor decoding

CRX version `0x100`, four Bayer planes and `ff01`/`ff02`/`ff03` marker headers
support lossless RAW and three-level C-RAW with horizontal tiles. Uniform
quantization values 4–43 are independently verified. Version `0x200` C-RAW
supports the `ff11`/`ff12`/`ff13` marker family, a single 14-bit Bayer tile and
its adaptive quantization map. C-RAW is quantized, so next to clipped highlights
its reconstruction can overshoot the sensor range by a few quantization steps
(up to 36 codes above 16383 in 17 CC0 files); those samples are clamped to the
CMP1 bit depth. A lossless sample outside the bit depth is still corrupt data.
Every other unverified coding variant returns an explicit unsupported error —
see the variant-status table above for the complete list. Header probing
validates the same coding variants and tile/plane layout as full decoding.

`crates/raw/tests/cr3_corpus.rs` checks every full-sensor sample against a
black-box reference, before cropping or colour processing. Public CC0 files
are downloaded by `cargo xtask corpus --download` and kept outside version
control. The exact inputs and reference hashes are:

| File | Input SHA-256 | Sensor FNV-1a (u16 little-endian) |
|---|---|---|
| Canon M50 RAW, raw.pixls.us 4657 | `1ba9ad6b51b315b88820eb0fe5fd7f52bc136ba37b1a5ca76825ebfe25b18058` | `62261f0ba81cfcd2` |
| Canon M50 C-RAW, raw.pixls.us 2663 | `15384b775867ec4c42b11882837f1e368cedc0561832ffab271221e6bb80be4c` | `9aacbfe66f505e1b` |
| Canon R100 RAW, raw.pixls.us 7896 | `6d83217d58a5e6d2dabcf470430e91a16676b60531c025b81fa7453fc74b0521` | `cbe5299ab9c52630` |
| Canon R100 C-RAW, raw.pixls.us 7897 | `0b66842b2fe00329ebc05fe8ab9357ddbbe1a0a2d2e69a2893102de7fdf5c942` | `341c706b37c38bbf` |
| Canon R8 RAW, raw.pixls.us 6585 | `7d5c6dbb11ff7e6ee58715d90a20f5d801e8b103f4711ff6ea329ceafcef254c` | `63c1c6d8d1eb2312` |
| Canon R8 C-RAW, raw.pixls.us 6587 | `df33cf394573645ce03dea1b2e9f0b5cc2b7734e9e3391ee7114151414cf2812` | `f836a663a795a375` |

Reference pixels were observed from the DNGLab 0.8.0 macOS arm64 release
binary; no decoder source was read or copied, and it is not shipped or linked.
The release ZIP SHA-256 was
`ee70805cb60f18d5ed62548c4e595f6cee0a33407e15358eb1766a46afc1c16e`.
All six sensor comparisons pass. R8 HEVC preview tracks are distinguished from
the primary Bayer RAW; the HEVC preview itself is not decoded. Missing corpus
files are reported as skipped by the tests.

The version `0x200` QP predictor and entropy framing were checked independently.
Controlled input mutations establish floor averaging of paired QP rows before
the nonlinear integer table, then multiplication, division by eight, addition
of the subband base and a minimum step of one. QP values 131–167 are verified;
unmeasured values are rejected, and the odd-QP-map-height rejection is tracked,
fix in flight on another branch (see the variant-status table). The remaining
unsupported families are enforced by `crates/raw/tests/cr3_variants.rs`. The
decoder bounds image dimensions, sample count, tile count, marker headers and
temporary allocations.
The full sensor buffer is allocated only after the first plane decodes, and
plane buffers grow row by row, so a small crafted file fails before reserving
memory for the image it claims.

## Colour limits

CR3 carries as-shot white balance but no measured camera matrix is imported
here. The existing guarded embedded-JPEG fitting path now includes CR3, with
both proxies oriented identically before gathering training pairs. A bounded
crop/translation registration of the JPEG is confirmed on held-out edge
directions before fitting colour; the existing held-out colour acceptance gates
are unchanged. No lens distortion correction is applied (LightCraft has no lens
profiles of its own yet), so lenses whose camera JPEG is strongly
distortion-corrected may fail the gates and keep the neutral matrix. Render
cache version 13 invalidates earlier thumbnails. All developed pixels still come
from the RAW sensor; a rejected colour fit retains the documented neutral
matrix fallback. See `camera-preview-colour.md` for the limits of this estimate.
