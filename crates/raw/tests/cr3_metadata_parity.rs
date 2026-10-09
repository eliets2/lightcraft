//! CR3 metadata parity: what LightCraft extracts per body vs the ExifTool ground truth.
//!
//! Ground truth was captured once with `exiftool -a -U -G1 -H -v3` (ExifTool 13.59) over the
//! gitignored CC0 corpus (`corpus/raw/cr3-*.cr3`, 110 files / 29 bodies); the dumps live outside
//! the repository under `D:\lightcraft-parity\cr3\exiftool-dumps\` and the expected values below
//! were generated from them by `tools/cr3_metadata_reference.py`. The test never runs ExifTool: it
//! re-derives LightCraft's fields through the same [`probe_info`] / [`decode`] path the importer
//! uses and compares them against the committed table, so it stays hermetic and fast. Rows whose
//! corpus file is absent are skipped (reported on stderr).
//!
//! Field map follows `docs/cr3-metadata-parity.md` (E-corpus-verification §6 priority order):
//! as-shot white balance, per-body black/white levels, crop/active area, developed size (aspect),
//! dual-pixel handling and the lens string. Known-absent fields are asserted absent so the table
//! documents reality instead of hiding it (today: the camera `AspectInfo` crop — the EOS R6 Mark
//! III ColorData v66 was parsed since cr3/p4-color-matrices), and [`KNOWN_DECODE_GAPS`] /
//! [`KNOWN_DEVELOPED_GAPS`] pin the places
//! where decoding still deviates — when one heals, the assert says what to regenerate.

use lightcraft_raw::{RawData, RawFormat, RawImage, RawInfo, decode, probe_info};
use std::path::{Path, PathBuf};

fn corpus() -> PathBuf {
    std::env::var_os("LIGHTCRAFT_CORPUS").map(PathBuf::from).unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus"))
}

/// One per-body (or per-variant) ground-truth row.
#[derive(Clone, Copy)]
struct Entry {
    /// Corpus file name under `corpus/raw`.
    file: &'static str,
    /// ExifTool `Model` (CMT1 IFD0 0x0110), the expected `metadata.model`.
    model: &'static str,
    /// ExifTool `ColorDataVersion` (maker note 0x4001 word 0); documents the per-body layout.
    color_data: u16,
    /// Dual Pixel RAW file (two full-size raw tracks; the second is a delta and must not win).
    dpr: bool,
    /// As-shot WB multipliers `[red, blue]` (green = 1) from ExifTool `WB_RGGBLevelsAsShot`
    /// (word offsets 71/85/105 per ColorData version). `None`: LightCraft must report none.
    wb: Option<[f32; 2]>,
    /// ExifTool `PerChannelBlackLevel` (0x149/0x157/0x16b/0x17f by version). `None`: LightCraft
    /// measures black from the samples (sanity-checked only).
    black: Option<[f32; 4]>,
    /// ExifTool `SpecularWhiteLevel` (0x31d/0x32b/0x281/0x295 by version). `None`: sample-derived.
    white: Option<f32>,
    /// ExifTool `SensorWidth`/`SensorHeight` (SensorInfo 0x00e0 words 1-2): full sensor dims.
    sensor: (u32, u32),
    /// ExifTool `ExifImageWidth`/`ExifImageHeight` (CMT1 0xa002/0xa003): developed size ground
    /// truth (the recommended crop; also pins the aspect information).
    developed: (u32, u32),
    /// ExifTool `AspectInfo` `CroppedImageWidth`/`CroppedImageHeight` (CMT3): the camera-applied
    /// aspect crop when an aspect mode is active, else (0, 0). Ground truth only: until it is
    /// implemented the decoder develops the full recommended crop (`ASPECT_APPLIED` documents that).
    aspect: (u32, u32),
    /// ExifTool `LensModel` (ExifIFD 0xa434; the CMT3 maker-note 0x0095 string agrees). Empty:
    /// fixed-lens bodies record no lens name, so absence is expected.
    lens: &'static str,
    /// ExifTool `CanonModelID` (0x4010): body identity for donation/coverage bookkeeping.
    model_id: u32,
}

/// Whether the decoder applies the CMT3 `AspectInfo` crop. `false` documents today's reality
/// (the camera aspect mode is ignored and the full recommended crop is developed); flip the
/// generator's `ASPECT_APPLIED` together with this when AspectInfo lands.
const ASPECT_APPLIED: bool = false;

// BEGIN GENERATED TABLE — tools/cr3_metadata_reference.py against ExifTool 13.59 dumps; do not edit by hand.
#[rustfmt::skip]
const ENTRIES: &[Entry] = &[
    Entry { file: "cr3-canon-eos-m200-mg-2231.cr3", model: "Canon EOS M200", color_data: 19, dpr: false, wb: Some([1.5703125, 1.2236328125]), black: Some([511.0, 510.0, 511.0, 511.0]), white: Some(11606.0), sensor: (6288, 4056), developed: (6000, 4000), aspect: (0, 0), lens: "EF-M22mm f/2 STM", model_id: 2066 },
    Entry { file: "cr3-canon-eos-r10-img-6310.cr3", model: "Canon EOS R10", color_data: 48, dpr: false, wb: Some([1.8583984375, 1.5751953125]), black: Some([2048.0, 2048.0, 2048.0, 2048.0]), white: Some(14338.0), sensor: (6192, 4060), developed: (6000, 4000), aspect: (0, 0), lens: "150-600mm F5-6.3 DG OS HSM | Contemporary 015", model_id: 2147484773 },
    Entry { file: "cr3-canon-eos-r50-v-img-0034.cr3", model: "Canon EOS R50 V", color_data: 65, dpr: false, wb: Some([1.7900390625, 1.6435546875]), black: Some([512.0, 512.0, 512.0, 512.0]), white: Some(11708.0), sensor: (6192, 4060), developed: (6000, 4000), aspect: (0, 0), lens: "RF28mm F2.8 STM", model_id: 2147484950 },
    Entry { file: "cr3-canon-eos-r5m2-aps-c-raw.cr3", model: "Canon EOS R5m2", color_data: 64, dpr: false, wb: Some([1.73828125, 1.9248046875]), black: Some([512.0, 512.0, 512.0, 512.0]), white: Some(14888.0), sensor: (5376, 3574), developed: (5088, 3392), aspect: (5088, 3392), lens: "RF135mm F1.8 L IS USM", model_id: 2147484822 },
    Entry { file: "cr3-canon-eos-r5m2-raw.cr3", model: "Canon EOS R5m2", color_data: 64, dpr: false, wb: Some([1.8349609375, 1.7626953125]), black: Some([512.0, 512.0, 512.0, 512.0]), white: Some(14888.0), sensor: (8480, 5650), developed: (8192, 5464), aspect: (0, 0), lens: "RF135mm F1.8 L IS USM", model_id: 2147484822 },
    Entry { file: "cr3-canon-eos-r6-mark-iii-img-4241.cr3", model: "Canon EOS R6 Mark III", color_data: 66, dpr: false, wb: Some([2.2753906, 1.4199219]), black: Some([512.0, 512.0, 512.0, 512.0]), white: Some(13995.0), sensor: (7144, 4760), developed: (6960, 4640), aspect: (0, 0), lens: "RF50mm F1.4 L VCM", model_id: 2147484952 },
    Entry { file: "cr3-canon-m50-raw.cr3", model: "Canon EOS M50", color_data: 16, dpr: false, wb: Some([1.0771484375, 1.8994140625]), black: Some([510.0, 511.0, 511.0, 511.0]), white: Some(11892.0), sensor: (6288, 4056), developed: (6000, 4000), aspect: (0, 0), lens: "EF-M18-150mm f/3.5-6.3 IS STM", model_id: 1042 },
    Entry { file: "cr3-canon-powershot-v1-img-1225.cr3", model: "Canon PowerShot V1", color_data: 65, dpr: false, wb: Some([1.8896484375, 1.4990234375]), black: Some([512.0, 512.0, 512.0, 512.0]), white: Some(13036.0), sensor: (5928, 3944), developed: (5760, 3840), aspect: (0, 0), lens: "", model_id: 2147484823 },
    Entry { file: "cr3-canon-r100-raw.cr3", model: "Canon EOS R100", color_data: 19, dpr: false, wb: Some([1.79296875, 1.744140625]), black: Some([2048.0, 2048.0, 2048.0, 2048.0]), white: Some(10439.0), sensor: (6288, 4056), developed: (6000, 4000), aspect: (0, 0), lens: "RF-S18-45mm F4.5-6.3 IS STM", model_id: 2147484824 },
    Entry { file: "cr3-canon-r8-raw.cr3", model: "Canon EOS R8", color_data: 48, dpr: false, wb: Some([1.8515625, 1.587890625]), black: Some([512.0, 512.0, 512.0, 512.0]), white: Some(14008.0), sensor: (6188, 4120), developed: (6000, 4000), aspect: (0, 0), lens: "EF24-105mm f/4L IS II USM", model_id: 2147484807 },
    Entry { file: "cr3-eos-1d-x-mark-iii-canon-eos-1d-x-mark-iii-raw-iso-100-nocrop-nodual.cr3", model: "Canon EOS-1D X Mark III", color_data: 32, dpr: false, wb: Some([1.59765625, 1.77734375]), black: Some([512.0, 512.0, 512.0, 512.0]), white: Some(14008.0), sensor: (5568, 3708), developed: (5472, 3648), aspect: (0, 0), lens: "EF24-70mm f/2.8L II USM", model_id: 2147484712 },
    Entry { file: "cr3-eos-200d-mark-ii-mg-3608.cr3", model: "Canon EOS 200D II", color_data: 18, dpr: false, wb: Some([1.9501953125, 1.53515625]), black: Some([2049.0, 2049.0, 2049.0, 2049.0]), white: Some(14338.0), sensor: (6288, 4056), developed: (6000, 4000), aspect: (0, 0), lens: "EF70-300mm f/4-5.6 IS II USM", model_id: 2147484726 },
    Entry { file: "cr3-eos-250d-canon250d-craw-3-2.cr3", model: "Canon EOS 250D", color_data: 18, dpr: false, wb: Some([1.841796875, 1.6357421875]), black: Some([511.0, 511.0, 511.0, 511.0]), white: Some(11606.0), sensor: (6288, 4056), developed: (6000, 4000), aspect: (0, 0), lens: "EF40mm f/2.8 STM", model_id: 2147484726 },
    Entry { file: "cr3-eos-250d-canon250d-raw-1-1.cr3", model: "Canon EOS 250D", color_data: 18, dpr: false, wb: Some([1.841796875, 1.6357421875]), black: Some([511.0, 511.0, 511.0, 511.0]), white: Some(11606.0), sensor: (6288, 4056), developed: (6000, 4000), aspect: (4000, 4000), lens: "EF40mm f/2.8 STM", model_id: 2147484726 },
    Entry { file: "cr3-eos-250d-canon250d-raw-19-9.cr3", model: "Canon EOS 250D", color_data: 18, dpr: false, wb: Some([1.841796875, 1.6357421875]), black: Some([511.0, 511.0, 511.0, 511.0]), white: Some(11606.0), sensor: (6288, 4056), developed: (6000, 4000), aspect: (6000, 3368), lens: "EF40mm f/2.8 STM", model_id: 2147484726 },
    Entry { file: "cr3-eos-250d-canon250d-raw-2-3.cr3", model: "Canon EOS 250D", color_data: 18, dpr: false, wb: Some([1.841796875, 1.6357421875]), black: Some([511.0, 511.0, 511.0, 511.0]), white: Some(11606.0), sensor: (6288, 4056), developed: (6000, 4000), aspect: (0, 0), lens: "EF40mm f/2.8 STM", model_id: 2147484726 },
    Entry { file: "cr3-eos-850d-img-1837.cr3", model: "Canon EOS 850D", color_data: 19, dpr: false, wb: Some([2.0517578125, 1.4951171875]), black: Some([511.0, 511.0, 511.0, 511.0]), white: Some(11606.0), sensor: (6288, 4056), developed: (6000, 4000), aspect: (0, 0), lens: "EF-S17-85mm f/4-5.6 IS USM", model_id: 2147484725 },
    Entry { file: "cr3-eos-90d-canon-90d-raw-iso-100.cr3", model: "Canon EOS 90D", color_data: 19, dpr: false, wb: Some([2.0478515625, 1.34375]), black: Some([512.0, 512.0, 512.0, 512.0]), white: Some(13036.0), sensor: (7128, 4732), developed: (6960, 4640), aspect: (0, 0), lens: "EF-S18-55mm f/3.5-5.6 IS", model_id: 2147484727 },
    Entry { file: "cr3-eos-kiss-m-img-2695.cr3", model: "Canon EOS KISS M", color_data: 16, dpr: false, wb: Some([1.5830078125, 1.3251953125]), black: Some([510.0, 510.0, 510.0, 509.0]), white: Some(11892.0), sensor: (6288, 4056), developed: (6000, 4000), aspect: (0, 0), lens: "", model_id: 1042 },
    Entry { file: "cr3-eos-m50-mark-ii-img-0051.cr3", model: "Canon EOS M50m2", color_data: 19, dpr: false, wb: Some([1.302734375, 1.521484375]), black: Some([2049.0, 2049.0, 2049.0, 2049.0]), white: Some(10439.0), sensor: (6288, 4056), developed: (6000, 4000), aspect: (0, 0), lens: "EF-M15-45mm f/3.5-6.3 IS STM", model_id: 2147484776 },
    Entry { file: "cr3-eos-m6-mark-ii-img-raw-1-1.cr3", model: "Canon EOS M6 Mark II", color_data: 19, dpr: false, wb: Some([1.5732421875, 2.0078125]), black: Some([2048.0, 2048.0, 2048.0, 2048.0]), white: Some(13660.0), sensor: (7128, 4732), developed: (6960, 4640), aspect: (4640, 4640), lens: "EF-M15-45mm f/3.5-6.3 IS STM", model_id: 2065 },
    Entry { file: "cr3-eos-m6-mark-ii-img-raw-3-2.cr3", model: "Canon EOS M6 Mark II", color_data: 19, dpr: false, wb: Some([1.568359375, 2.01171875]), black: Some([2048.0, 2048.0, 2048.0, 2048.0]), white: Some(10852.0), sensor: (7128, 4732), developed: (6960, 4640), aspect: (0, 0), lens: "EF-M15-45mm f/3.5-6.3 IS STM", model_id: 2065 },
    Entry { file: "cr3-eos-r-canon-eos-r-raw-iso-100.cr3", model: "Canon EOS R", color_data: 17, dpr: false, wb: Some([1.9541015625, 1.544921875]), black: Some([511.0, 510.0, 511.0, 510.0]), white: Some(14008.0), sensor: (6888, 4546), developed: (6720, 4480), aspect: (0, 0), lens: "RF85mm F2 MACRO IS STM", model_id: 2147484708 },
    Entry { file: "cr3-eos-r-canon-eos-r-raw-iso-100-crop-nodual.cr3", model: "Canon EOS R", color_data: 17, dpr: false, wb: Some([1.97265625, 1.546875]), black: Some([511.0, 510.0, 511.0, 510.0]), white: Some(14008.0), sensor: (4352, 2850), developed: (4176, 2784), aspect: (4176, 2784), lens: "", model_id: 2147484708 },
    Entry { file: "cr3-eos-r-canon-eos-r-raw-iso-100-dual-pixel-on.cr3", model: "Canon EOS R", color_data: 17, dpr: true, wb: Some([1.9658203125, 1.533203125]), black: Some([510.0, 510.0, 510.0, 509.0]), white: Some(14008.0), sensor: (6888, 4546), developed: (6720, 4480), aspect: (0, 0), lens: "RF85mm F2 MACRO IS STM", model_id: 2147484708 },
    Entry { file: "cr3-eos-r3-c3a1200.cr3", model: "Canon EOS R3", color_data: 34, dpr: false, wb: Some([2.0234375, 1.701171875]), black: Some([2047.0, 2047.0, 2047.0, 2047.0]), white: Some(14351.0), sensor: (6192, 4152), developed: (6000, 4000), aspect: (0, 0), lens: "RF100-400mm F5.6-8 IS USM", model_id: 2147484752 },
    Entry { file: "cr3-eos-r5-canon-eos-r5-raw-iso-100-crop-nodual.cr3", model: "Canon EOS R5", color_data: 33, dpr: false, wb: Some([1.5400390625, 2.4443359375]), black: Some([510.0, 510.0, 510.0, 510.0]), white: Some(14008.0), sensor: (5248, 3510), developed: (5088, 3392), aspect: (5088, 3392), lens: "50mm F1.4 DG HSM | Art 014", model_id: 2147484705 },
    Entry { file: "cr3-eos-r5-canon-eos-r5-raw-iso-100-nocrop-dual.cr3", model: "Canon EOS R5", color_data: 33, dpr: true, wb: Some([1.7177734375, 2.1376953125]), black: Some([512.0, 512.0, 512.0, 512.0]), white: Some(14008.0), sensor: (8352, 5586), developed: (8192, 5464), aspect: (0, 0), lens: "50mm F1.4 DG HSM | Art 014", model_id: 2147484705 },
    Entry { file: "cr3-eos-r5-canon-eos-r5-raw-iso-100-nocrop-nodual.cr3", model: "Canon EOS R5", color_data: 33, dpr: false, wb: Some([1.6201171875, 2.2607421875]), black: Some([510.0, 510.0, 510.0, 510.0]), white: Some(14008.0), sensor: (8352, 5586), developed: (8192, 5464), aspect: (0, 0), lens: "50mm F1.4 DG HSM | Art 014", model_id: 2147484705 },
    Entry { file: "cr3-eos-r50-canon-r50-iso100-raw-electronicshutter.cr3", model: "Canon EOS R50", color_data: 48, dpr: false, wb: Some([1.7265625, 1.6953125]), black: Some([512.0, 512.0, 512.0, 512.0]), white: Some(11606.0), sensor: (6192, 4060), developed: (6000, 4000), aspect: (0, 0), lens: "EF-S18-55mm f/3.5-5.6 IS", model_id: 2147484800 },
    Entry { file: "cr3-eos-r6-canon-eos-r6-raw-iso-100-crop-nodual.cr3", model: "Canon EOS R6", color_data: 33, dpr: false, wb: Some([1.8125, 1.630859375]), black: Some([512.0, 512.0, 512.0, 512.0]), white: Some(14008.0), sensor: (3584, 2386), developed: (3408, 2272), aspect: (3408, 2272), lens: "RF70-200mm F2.8 L IS USM", model_id: 2147484755 },
    Entry { file: "cr3-eos-r6-canon-eos-r6-raw-iso-100-nocrop-nodual.cr3", model: "Canon EOS R6", color_data: 33, dpr: false, wb: Some([1.8056640625, 1.638671875]), black: Some([512.0, 512.0, 512.0, 512.0]), white: Some(14008.0), sensor: (5568, 3708), developed: (5472, 3648), aspect: (0, 0), lens: "RF70-200mm F2.8 L IS USM", model_id: 2147484755 },
    Entry { file: "cr3-eos-r6-mark-ii-canon-r6m2-img-0346-raw-normal-full.cr3", model: "Canon EOS R6m2", color_data: 48, dpr: false, wb: Some([2.146484375, 1.29296875]), black: Some([512.0, 512.0, 512.0, 512.0]), white: Some(14008.0), sensor: (6188, 4120), developed: (6000, 4000), aspect: (0, 0), lens: "EF50mm f/1.8 II", model_id: 2147484801 },
    Entry { file: "cr3-eos-r6-mark-ii-canon-r6m2-img-0349-raw-dual-crop.cr3", model: "Canon EOS R6m2", color_data: 48, dpr: true, wb: Some([2.09375, 1.330078125]), black: Some([512.0, 512.0, 512.0, 512.0]), white: Some(14008.0), sensor: (3936, 2612), developed: (3744, 2496), aspect: (3744, 2496), lens: "EF50mm f/1.8 II", model_id: 2147484801 },
    Entry { file: "cr3-eos-r7-443a0157.cr3", model: "Canon EOS R7", color_data: 48, dpr: false, wb: Some([1.7041015625, 1.6591796875]), black: Some([512.0, 511.0, 511.0, 512.0]), white: Some(13036.0), sensor: (7128, 4732), developed: (6960, 4640), aspect: (0, 0), lens: "RF-S18-150mm F3.5-6.3 IS STM", model_id: 2147484772 },
    Entry { file: "cr3-eos-r8-raw-full-frame-dual-pixel.cr3", model: "Canon EOS R8", color_data: 48, dpr: true, wb: Some([1.8583984375, 1.5927734375]), black: Some([512.0, 512.0, 512.0, 512.0]), white: Some(14008.0), sensor: (6188, 4120), developed: (6000, 4000), aspect: (0, 0), lens: "EF24-105mm f/4L IS II USM", model_id: 2147484807 },
    Entry { file: "cr3-eos-rebel-sl3-sedona2020-sl3-mg-1061.cr3", model: "Canon EOS Rebel SL3", color_data: 18, dpr: false, wb: Some([1.841796875, 1.6025390625]), black: Some([511.0, 511.0, 511.0, 511.0]), white: Some(11606.0), sensor: (6288, 4056), developed: (6000, 4000), aspect: (0, 0), lens: "", model_id: 2147484726 },
    Entry { file: "cr3-eos-rp-canon-eos-rp-raw-iso-100-crop-nodual.cr3", model: "Canon EOS RP", color_data: 18, dpr: false, wb: Some([1.7294921875, 2.115234375]), black: Some([513.0, 513.0, 513.0, 513.0]), white: Some(14558.0), sensor: (4032, 2656), developed: (3888, 2592), aspect: (3888, 2592), lens: "RF50mm F1.8 STM", model_id: 2147484723 },
    Entry { file: "cr3-eos-rp-canon-eos-rp-raw-iso-100-nocrop-nodual.cr3", model: "Canon EOS RP", color_data: 18, dpr: false, wb: Some([1.7470703125, 2.0478515625]), black: Some([513.0, 513.0, 513.0, 513.0]), white: Some(14558.0), sensor: (6384, 4224), developed: (6240, 4160), aspect: (0, 0), lens: "RF50mm F1.8 STM", model_id: 2147484723 },
    Entry { file: "cr3-powershot-g5-x-mark-ii-canon-powershot-g5-x-mark-ii-raw-1-1.cr3", model: "Canon PowerShot G5 X Mark II", color_data: 19, dpr: false, wb: Some([2.515625, 1.7265625]), black: Some([2048.0, 2048.0, 2048.0, 2048.0]), white: Some(14335.0), sensor: (5632, 3710), developed: (5472, 3648), aspect: (3648, 3648), lens: "", model_id: 2052 },
    Entry { file: "cr3-powershot-g5-x-mark-ii-canon-powershot-g5-x-mark-ii-raw-3-2.cr3", model: "Canon PowerShot G5 X Mark II", color_data: 19, dpr: false, wb: Some([2.509765625, 1.662109375]), black: Some([2048.0, 2048.0, 2048.0, 2048.0]), white: Some(14335.0), sensor: (5632, 3710), developed: (5472, 3648), aspect: (0, 0), lens: "", model_id: 2052 },
    Entry { file: "cr3-powershot-g7-x-mark-iii-img-0253.cr3", model: "Canon PowerShot G7 X Mark III", color_data: 19, dpr: false, wb: Some([2.2509765625, 1.865234375]), black: Some([2048.0, 2048.0, 2048.0, 2048.0]), white: Some(14335.0), sensor: (5632, 3710), developed: (5472, 3648), aspect: (0, 0), lens: "", model_id: 2056 },
    Entry { file: "cr3-powershot-sx70-hs-img-1000.cr3", model: "Canon PowerShot SX70 HS", color_data: 18, dpr: false, wb: Some([1.546875, 1.7626953125]), black: Some([512.0, 512.0, 512.0, 512.0]), white: Some(12500.0), sensor: (5344, 3950), developed: (5184, 3888), aspect: (5184, 3888), lens: "", model_id: 2053 },
];
// END GENERATED TABLE

/// Corpus files that (persistently) fail full decode; `(file, expected-error substring)`. Their
/// header metadata still gates above. When a decode heals, the assert below fails with
/// instructions: drop the row here and regenerate the table with
/// `tools/cr3_metadata_reference.py <dump-dir> --update` so the row's black/white levels gate too.
/// Empty today: the one observed failure (PowerShot V1, "QP coordinate outside map", once in ~a
/// dozen full-sweep runs) coincided with a stale shared-target rlib built from another worktree's
/// in-progress `vendor/crx.rs` — after a rebuild from this branch's source, 13+ sweeps and the
/// full suite (incl. `corpus_raw_decodes`) pass; see `docs/cr3-metadata-parity.md` (F1).
const KNOWN_DECODE_GAPS: &[(&str, &str)] = &[];

/// Corpus files whose developed size (`crop.clipped(active_area)`) deviates from the camera's own
/// recommendation; `(file, LightCraft's current size)`. The ground truth stays in the table's
/// `developed` field; the assert pins the documented deviation so a silent change cannot slip in.
const KNOWN_DEVELOPED_GAPS: &[(&str, (u32, u32))] = &[
    // EOS R5m2 APS-C: the IAD1 active area is 5112×3408, but CMT1 ExifImageWidth/Height
    // (0xa002/0xa003) and the AspectInfo crop both say 5088×3392 — 24×16 px of border are kept.
    ("cr3-canon-eos-r5m2-aps-c-raw.cr3", (5112, 3408)),
];

fn path_of(entry: &Entry) -> PathBuf {
    corpus().join("raw").join(entry.file)
}

/// Decode, retrying once on failure: the CRX decoder is rayon-parallel and very rarely fails
/// spuriously on a good file (see [`KNOWN_DECODE_GAPS`]); a retry keeps this metadata gate from
/// tripping over that flake, while a reproducible failure still fails with its error.
fn decode_retrying(entry: &Entry, bytes: &[u8]) -> lightcraft_raw::Result<RawImage> {
    decode(bytes).or_else(|first| {
        eprintln!("note: {}: decode failed once ({first}); retrying", entry.file);
        decode(bytes).inspect_err(|second| {
            assert_eq!(first.to_string(), second.to_string(), "{}: decode errors differ between retries", entry.file);
        })
    })
}

/// Header fields for every present corpus file; samples too unless the file is a documented decode
/// gap — there the recorded failure is asserted, so a fix or a changed failure both force
/// reconciliation with the table.
fn load(entry: &Entry) -> Option<(RawInfo, Option<RawImage>)> {
    let path = path_of(entry);
    let Ok(bytes) = std::fs::read(&path) else {
        eprintln!("skip: {} absent", path.display());
        return None;
    };
    let info = probe_info(&bytes).unwrap_or_else(|e| panic!("{}: probe failed: {e}", entry.file));
    assert_eq!(info.format, RawFormat::Cr3, "{}", entry.file);
    let image = match KNOWN_DECODE_GAPS.iter().find(|(f, _)| *f == entry.file) {
        Some((_, expected)) => {
            let msg = format!(
                "{} now decodes: remove it from KNOWN_DECODE_GAPS and regenerate the table with tools/cr3_metadata_reference.py --update so its black/white levels gate again",
                entry.file
            );
            let err = decode_retrying(entry, &bytes).expect_err(&msg).to_string();
            assert!(err.contains(expected), "{}: decode-gap error changed: {err:?} (expected {expected:?}); update KNOWN_DECODE_GAPS", entry.file);
            eprintln!("note: {}: documented decode gap: {err}", entry.file);
            None
        }
        None => Some(decode_retrying(entry, &bytes).unwrap_or_else(|e| panic!("{}: decode failed: {e}", entry.file))),
    };
    Some((info, image))
}

fn close(actual: f32, expected: f32) -> bool {
    (actual - expected).abs() <= expected.abs() * 1e-4
}

/// Parity of every committed ground-truth row that has its corpus file present.
#[test]
fn metadata_matches_exiftool_ground_truth_per_body() {
    let mut checked = 0usize;
    for entry in ENTRIES {
        let Some((info, image)) = load(entry) else { continue };
        checked += 1;
        let file = entry.file;

        // Header-side fields (what the importer records).
        assert_eq!((info.width, info.height), (entry.sensor.0 as usize, entry.sensor.1 as usize), "{file}: sensor dims");
        assert_eq!(info.metadata.model.as_deref(), Some(entry.model), "{file}: model string");
        match (entry.wb, info.wb_multipliers) {
            (Some([r, b]), Some(actual)) => {
                assert!(close(actual[0], r) && close(actual[2], b) && close(actual[1], 1.0), "{file}: as-shot WB {actual:?} vs [{r}, 1, {b}]");
            }
            (None, None) => {}
            (None, Some(actual)) => panic!("{file}: unexpected as-shot WB {actual:?} (documented absent)"),
            (Some(_), None) => panic!("{file}: as-shot WB absent, expected [{:?}]", entry.wb.unwrap()),
        }
        if entry.lens.is_empty() {
            // Fixed-lens PowerShots carry neither ExifIFD LensModel (0xa434, empty) nor the
            // CMT3 0x0095 string: the lens name is expected to be absent.
            assert_eq!(info.metadata.lens_model, None, "{file}: unexpected lens string");
        } else {
            assert_eq!(info.metadata.lens_model.as_deref(), Some(entry.lens), "{file}: lens string");
        }
        assert!(
            info.active_area.width > 0
                && info.active_area.width <= info.width
                && info.active_area.height > 0
                && info.active_area.height <= info.height,
            "{file}: active area {:?} outside sensor",
            info.active_area
        );
        let crop = info.crop.clipped(info.active_area.width, info.active_area.height);
        assert!(crop.width >= 2 && crop.height >= 2, "{file}: default crop {crop:?} is degenerate");

        // Developed size: the camera's recommendation (CMT1 ExifImageWidth/Height 0xa002/0xa003),
        // except documented gaps; always sane (camera aspect ratio, inside the sensor).
        let expected_dev = if ASPECT_APPLIED && entry.aspect != (0, 0) { entry.aspect } else { entry.developed };
        let dev = info.developed_size();
        let ratio = |w: usize, h: usize| w as f32 / h as f32;
        assert!(
            (ratio(dev.0, dev.1) - ratio(expected_dev.0 as usize, expected_dev.1 as usize)).abs()
                <= 0.01 * ratio(expected_dev.0 as usize, expected_dev.1 as usize),
            "{file}: developed {dev:?} has the wrong aspect vs {expected_dev:?}"
        );
        assert!(dev.0 <= info.width && dev.1 <= info.height, "{file}: developed {dev:?} exceeds the sensor");
        match KNOWN_DEVELOPED_GAPS.iter().find(|(f, _)| *f == entry.file) {
            Some((_, lc)) => assert_eq!(
                dev,
                (lc.0 as usize, lc.1 as usize),
                "{file}: documented developed-size gap moved (ground truth {expected_dev:?}; see KNOWN_DEVELOPED_GAPS)"
            ),
            None => {
                assert_eq!(dev, (expected_dev.0 as usize, expected_dev.1 as usize), "{file}: developed size (camera aspect crop {:?})", entry.aspect)
            }
        }

        // Sample-side fields: black/white levels need the decoded pixels.
        if let Some(image) = image {
            let RawData::U16(_) = &image.data else { panic!("{file}: integer sensor data expected") };
            let black_max = image.black.values.iter().cloned().fold(0.0f32, f32::max);
            assert!(image.black.values.iter().all(|b| b.is_finite() && *b >= 0.0), "{file}: black levels");
            assert!(image.white.iter().all(|w| w.is_finite() && *w > black_max), "{file}: white {:?} must exceed black {black_max}", image.white);
            if let (Some(black), Some(white)) = (entry.black, entry.white) {
                let mut actual = image.black.values.clone();
                actual.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                let mut expected: Vec<f32> = black.into_iter().collect();
                expected.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                assert_eq!(actual, expected, "{file}: per-channel black levels");
                assert_eq!(image.white[0], white, "{file}: white level");
            } else {
                panic!("{file}: sample-derived levels are no longer expected — every corpus ColorData version (v16–v66) is parsed");
            }
        }
        let _ = entry.dpr; // documented per row; the sensor-dims assert above pins the main track
    }
    if checked == 0 {
        eprintln!("skip: no corpus CR3 files present (set LIGHTCRAFT_CORPUS or corpus/raw)");
    }
}

/// One strict-JSON line per entry with LightCraft's extracted fields — the extraction-side dump
/// the parity report (`docs/cr3-metadata-parity.md`) is written from. Decode failures are dumped
/// as `"error"` instead of failing, so one run sweeps every entry. Ignored by default:
/// `cargo test -p lightcraft-raw --test cr3_metadata_parity dump -- --ignored --nocapture`.
#[test]
#[ignore = "extraction-side dump for docs/cr3-metadata-parity.md, not a gate"]
fn dump_lightcraft_fields() {
    let jq = |s: &str| format!("{s:?}");
    for entry in ENTRIES {
        let Ok(bytes) = std::fs::read(path_of(entry)) else {
            eprintln!("skip: {} absent", entry.file);
            continue;
        };
        let Ok(info) = probe_info(&bytes) else {
            println!("{{\"file\": {}, \"error\": \"probe failed\"}}", jq(entry.file));
            continue;
        };
        let wb = match info.wb_multipliers {
            Some([r, g, b]) => format!("[{r}, {g}, {b}]"),
            None => "null".to_string(),
        };
        let (black, white, error) = match decode_retrying(entry, &bytes) {
            Ok(image) => (
                format!("[{}]", image.black.values.iter().map(|b| format!("{b}")).collect::<Vec<_>>().join(", ")),
                image.white.first().map(|w| format!("{w}")).unwrap_or_else(|| "null".to_string()),
                "null".to_string(),
            ),
            Err(e) => ("null".to_string(), "null".to_string(), jq(&e.to_string())),
        };
        println!(
            "{{\"file\": {}, \"model\": {}, \"modelId\": {}, \"colorData\": {}, \"dpr\": {}, \"wb\": {}, \"black\": {}, \"white\": {}, \"sensor\": [{}, {}], \"active\": [{}, {}, {}, {}], \"crop\": [{}, {}, {}, {}], \"developed\": [{}, {}], \"lens\": {}, \"error\": {}}}",
            jq(entry.file),
            jq(info.metadata.model.as_deref().unwrap_or("")),
            entry.model_id,
            entry.color_data,
            entry.dpr,
            wb,
            black,
            white,
            info.width,
            info.height,
            info.active_area.x,
            info.active_area.y,
            info.active_area.width,
            info.active_area.height,
            info.crop.x,
            info.crop.y,
            info.crop.width,
            info.crop.height,
            info.developed_size().0,
            info.developed_size().1,
            jq(info.metadata.lens_model.as_deref().unwrap_or("")),
            error,
        );
    }
}
