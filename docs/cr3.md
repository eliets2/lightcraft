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

## Verified sensor decoding

CRX version `0x100`, four Bayer planes and `ff01`/`ff02`/`ff03` marker headers
support lossless RAW and three-level C-RAW with horizontal tiles. Uniform
quantization values 4–43 are independently verified. Version `0x200` C-RAW
supports the `ff11`/`ff12`/`ff13` marker family, a single 14-bit Bayer tile and
its adaptive quantization map, including odd QP-map heights: the map stores
`ceil(height/4)` rows, and when that count is odd the unpaired last row is
averaged with itself (which keeps its own value). Adaptive QP values 131–168
are verified. C-RAW is quantized, so next to clipped highlights
its reconstruction can overshoot the sensor range by a few quantization steps
(up to 36 codes above 16383 in 17 CC0 files); those samples are clamped to the
CMP1 bit depth. A lossless sample outside the bit depth is still corrupt data.
Other unverified coding variants return explicit unsupported errors. Header probing validates the same coding variants and
tile/plane layout as full decoding.

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
| Canon R5 Mark II C-RAW, `Canon EOS R5m2/CRAW.CR3` | `b3c08a5c7a97299dfd58896a12b939e7b43ec78682761812524825e6e5d087be` | `af32b45357b534e5` |
| Canon R6 Mark III C-RAW, `Canon EOS R6 Mark III/IMG_4244.CR3` | `7e1a5d84297a52853b0bbe5a5c03a5d4f5b93e00b7020271523240f9cabfe50e` | `ffd4bbb552ed0b47` |
| Canon 1D X Mark III C-RAW, `EOS-1D X Mark III/…_CRAW_ISO_100_nocrop_nodual.CR3` | `e5755d1414a2dd51fb0120b272058f00b0199861646070c00304d927eb119560` | `3472dbe7a6661a61` |
| Canon R5 C-RAW Dual Pixel, `EOS R5/…_CRAW_ISO_100_nocrop_dual.CR3` | `00b80633af35c0f85b0299a1d2ea87c6788a9007a1df8a3c5c05ea487b2549e1` | `e19b885c864f97e1` |
| Canon R5 C-RAW, `EOS R5/…_CRAW_ISO_100_nocrop_nodual.CR3` | `2deae46b816e56ba91ae7a3892ca121290e12d5b22afb11f5b05a3d8f09f759f` | `db6405021fd7ba92` |
| Canon R50 C-RAW (electronic shutter), `EOS R50/Canon_R50_ISO100_cRAW_electronicShutter.CR3` | `38fc458f2f74d0cfa04afd0ce513b40797080079f0786bedbfc55c39cc28d7d8` | `de7ba40253854e75` |
| Canon R50 C-RAW (mechanical shutter), `EOS R50/Canon_R50_ISO100_cRAW_mechanicShutter.CR3` | `b79af9ffa184dd740e8af7a9f451f7150053d7e456b3435ca2e43bbbf5f65026` | `035400755fe5a2a7` |
| Canon R6 C-RAW (crop), `EOS R6/…_CRAW_ISO_100_crop_nodual.CR3` | `74abb0a113d075ad9887a058082f40dd2a938c4813a08474d82356f11a027778` | `b1c4a2eef4788db3` |
| Canon R6 C-RAW, `EOS R6/…_CRAW_ISO_100_nocrop_nodual.CR3` | `4775df76433b1e5844bc7a42b95d93efc8438c0a4cd9681e120e23fe3e3007e8` | `d3e0567e1ffb07f4` |
| Canon R6 Mark II C-RAW (Dual Pixel crop), `EOS R6 Mark II/canon-r6m2-img_0356-craw-dual-crop.cr3` | `deed19ea7b5178c70f64710c97bcdc9d21f6610c19b42a2310223e51034d4fc8` | `be43d848a46f60f5` |
| Canon R6 Mark II C-RAW (crop), `EOS R6 Mark II/canon-r6m2-img_0359-craw-normal-crop.cr3` | `51ae1e673d2801d8a54f33dd8669c43034c506e2d355e75b26c0648e2f14a431` | `f491c769d297ec65` |
| Canon R7 C-RAW, `EOS R7/443A0159.CR3` | `10a18e3f01ca9cd93408496d3853388ef140830548bfeed4237e40f967eb9d5c` | `e972065c59294bf1` |
| Canon R7 C-RAW, `EOS R7/443A0161.CR3` | `c01cee1b598120668370892c16a73da915f2aaec8169c5c60a96efd98d9aa220` | `c6e0dd451046c484` |

Reference pixels for the first six files were observed from the DNGLab 0.8.0
macOS arm64 release binary; the thirteen odd-QP-map files were observed from
the LibRaw 0.22.2 Win64 release binaries (`unprocessed_raw.exe -q -T`, a
full-sensor 16-bit dump with no black subtraction or scaling), compared sample
for sample over the entire sensor including masked borders. No decoder source
was read or copied, and no oracle is shipped or linked. The release archives
had SHA-256 `ee70805cb60f18d5ed62548c4e595f6cee0a33407e15358eb1766a46afc1c16e`
(DNGLab) and `ac64fa12bb00a7581332d4c6ab918c0533fb3f119d6b668d47a6875410dca948`
(LibRaw-0.22.2-Win64.zip). All nineteen sensor comparisons pass. R8 HEVC
preview tracks are distinguished from the primary Bayer RAW; the HEVC preview
itself is not decoded. Missing corpus files are reported as skipped by the
tests. The EOS R6 Mark III stores a ColorData version whose layout is not yet
imported, so its as-shot white balance is absent (a documented colour limit);
its black/white levels fall back to measurement and remain validated.

The version `0x200` QP predictor and entropy framing were checked independently.
Controlled input mutations establish floor averaging of paired QP rows before
the nonlinear integer table, then multiplication, division by eight, addition
of the subband base and a minimum step of one. QP values 131–168 are verified
(the R50 files use 168 across the map); larger values and truncated maps are
rejected, and a map with zero or oversized dimensions is still corrupt. The
odd-height last row was measured against the oracle on all thirteen files:
both plausible policies (averaging the row with itself, or skipping the
average) keep the row's own value, and the samples match bit-exactly.
Partial-subband and
rounded-plane coding, vertical C-RAW tiling and adaptive low-frequency gains
remain unsupported. The decoder bounds image dimensions, sample count,
tile count, marker headers and temporary allocations. The full sensor buffer is
allocated only after the first plane decodes, and plane buffers grow row by row,
so a small crafted file fails before reserving memory for the image it claims.

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
