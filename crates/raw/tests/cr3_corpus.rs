//! Exact sensor-pixel regressions, not embedded-JPEG comparisons.
//!
//! Public-domain inputs from raw.pixls.us, kept in the gitignored corpus/raw.
//! Reference pixels were independently obtained by running decoder release
//! executables as black-box oracles: the DNGLab v0.8.0 macOS arm64 build
//! (`analyze --raw-pixel`) for the six M50/R100/R8 fixtures, and the LibRaw
//! 0.22.2 Win64 build (`unprocessed_raw.exe -q -T`, a full-sensor 16-bit dump
//! without black subtraction or scaling) for the thirteen odd-QP-map fixtures.
//! Their sources and camera matrices were not used. Canonical FNV-1a hashes
//! below are over decoded u16 samples in row-major, little-endian order,
//! including masked borders. Oracle release archives:
//! DNGLab sha256 ee70805cb60f18d5ed62548c4e595f6cee0a33407e15358eb1766a46afc1c16e,
//! LibRaw-0.22.2-Win64.zip sha256 ac64fa12bb00a7581332d4c6ab918c0533fb3f119d6b668d47a6875410dca948.

use lightcraft_raw::{RawData, RawFormat, decode, probe_info};
use std::path::{Path, PathBuf};

fn corpus() -> PathBuf {
    std::env::var_os("LIGHTCRAFT_CORPUS").map(PathBuf::from).unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus"))
}

fn check(name: &str, hash: u64, sensor: (usize, usize), developed: (usize, usize), wb: bool) {
    let path = corpus().join("raw").join(name);
    let Ok(bytes) = std::fs::read(&path) else {
        eprintln!("skip: {} absent", path.display());
        return;
    };
    let raw = decode(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
    assert_eq!(raw.format, RawFormat::Cr3);
    raw.validate().unwrap();
    assert_eq!((raw.width, raw.height, raw.cpp), (sensor.0, sensor.1, 1));
    assert_eq!(raw.info().developed_size(), developed);
    assert_eq!(raw.info(), probe_info(&bytes).unwrap());
    let RawData::U16(samples) = &raw.data else { panic!("{name}: integer sensor data expected") };
    let actual =
        samples.iter().flat_map(|v| v.to_le_bytes()).fold(0xcbf2_9ce4_8422_2325u64, |h, byte| (h ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3));
    assert_eq!(actual, hash, "{name}: sensor pixels differ from the independent reference");
    assert_eq!(raw.wb_multipliers.is_some(), wb, "{name}: as-shot white balance presence differs");
    assert!(raw.white_at(0) > raw.black.mean(), "{name}: invalid black/white levels");
}

#[test]
fn canon_m50_lossless_sensor_is_exact() {
    // raw.pixls.us/getfile.php/4657; SHA256
    // 1ba9ad6b51b315b88820eb0fe5fd7f52bc136ba37b1a5ca76825ebfe25b18058
    check("cr3-canon-m50-raw.cr3", 0x6226_1f0b_a81c_fcd2, (6288, 4056), (6000, 4000), true);
}

#[test]
fn canon_r100_lossless_sensor_is_exact() {
    // raw.pixls.us/getfile.php/7896; SHA256
    // 6d83217d58a5e6d2dabcf470430e91a16676b60531c025b81fa7453fc74b0521
    check("cr3-canon-r100-raw.cr3", 0xcbe5_299a_b9c5_2630, (6288, 4056), (6000, 4000), true);
}

#[test]
fn canon_m50_craw_sensor_is_exact() {
    // raw.pixls.us/getfile.php/2663; SHA256
    // 15384b775867ec4c42b11882837f1e368cedc0561832ffab271221e6bb80be4c
    // The camera was set to 1:1: the file's AspectInfo is a 4000 x 4000 square at x = 1000 inside the 6000 x 4000
    // image (the archive's "3:2" label is wrong). The sensor samples are unchanged; only the crop is.
    check("cr3-canon-m50-craw.cr3", 0x9aac_bfe6_6f50_5e1b, (6288, 4056), (4000, 4000), true);
}

#[test]
fn canon_r100_craw_sensor_is_exact() {
    // raw.pixls.us/getfile.php/7897; SHA256
    // 0b66842b2fe00329ebc05fe8ab9357ddbbe1a0a2d2e69a2893102de7fdf5c942
    check("cr3-canon-r100-craw.cr3", 0x341c_706b_37c3_8bbf, (6288, 4056), (6000, 4000), true);
}

#[test]
fn canon_r8_lossless_sensor_is_exact() {
    // raw.pixls.us/getfile.php/6585; SHA256
    // 7d5c6dbb11ff7e6ee58715d90a20f5d801e8b103f4711ff6ea329ceafcef254c
    check("cr3-canon-r8-raw.cr3", 0x63c1_c6d8_d1eb_2312, (6188, 4120), (6000, 4000), true);
}

#[test]
fn canon_r8_craw_sensor_is_exact() {
    // raw.pixls.us/getfile.php/6587; SHA256
    // df33cf394573645ce03dea1b2e9f0b5cc2b7734e9e3391ee7114151414cf2812
    check("cr3-canon-r8-craw.cr3", 0xf836_a663_a795_a375, (6188, 4120), (6000, 4000), true);
}

/// Version `0x200` C-RAW files whose `ceil(height/4)` QP-map rows are ODD (sensor heights
/// divisible by 4 but not by 8). All thirteen failed with "adaptive QP map with an odd
/// height" before the last row was averaged with itself; every reference here matched the
/// LibRaw 0.22.2 oracle sample for sample over the entire full sensor, masked borders
/// included. The R50 files also carry adaptive QP value 168 (quantValue 44), measured as
/// the next entry of the integer quantization sequence.
/// The EOS R6 Mark III ColorData version 66 parses (v12 layout), so its as-shot white balance
/// is asserted present.

#[test]
fn canon_r5_mark_ii_craw_odd_qp_map_is_exact() {
    // raw.pixls.us/data/Canon/Canon%20EOS%20R5m2/CRAW.CR3; SHA256
    // b3c08a5c7a97299dfd58896a12b939e7b43ec78682761812524825e6e5d087be
    check("cr3-canon-eos-r5m2-craw.cr3", 0xaf32_b453_57b5_34e5, (8480, 5650), (8192, 5464), true);
}

#[test]
fn canon_r6_mark_iii_craw_odd_qp_map_is_exact() {
    // raw.pixls.us/data/Canon/Canon%20EOS%20R6%20Mark%20III/IMG_4244.CR3; SHA256
    // 7e1a5d84297a52853b0bbe5a5c03a5d4f5b93e00b7020271523240f9cabfe50e
    check("cr3-canon-eos-r6-mark-iii-img-4244.cr3", 0xffd4_bbb5_52ed_0b47, (4512, 2994), (4320, 2880), true);
}

#[test]
fn canon_1dx_mark_iii_craw_odd_qp_map_is_exact() {
    // raw.pixls.us/data/Canon/EOS-1D%20X%20Mark%20III/Canon_EOS-1D_X_Mark_III_CRAW_ISO_100_nocrop_nodual.CR3;
    // SHA256 e5755d1414a2dd51fb0120b272058f00b0199861646070c00304d927eb119560
    check("cr3-eos-1d-x-mark-iii-canon-eos-1d-x-mark-iii-craw-iso-100-nocrop-nodual.cr3", 0x3472_dbe7_a666_1a61, (5568, 3708), (5472, 3648), true);
}

#[test]
fn canon_r5_craw_dual_pixel_odd_qp_map_is_exact() {
    // raw.pixls.us/data/Canon/EOS%20R5/Canon_EOS_R5_CRAW_ISO_100_nocrop_dual.CR3; SHA256
    // 00b80633af35c0f85b0299a1d2ea87c6788a9007a1df8a3c5c05ea487b2549e1
    check("cr3-eos-r5-canon-eos-r5-craw-iso-100-nocrop-dual.cr3", 0xe19b_885c_864f_97e1, (8352, 5586), (8192, 5464), true);
}

#[test]
fn canon_r5_craw_odd_qp_map_is_exact() {
    // raw.pixls.us/data/Canon/EOS%20R5/Canon_EOS_R5_CRAW_ISO_100_nocrop_nodual.CR3; SHA256
    // 2deae46b816e56ba91ae7a3892ca121290e12d5b22afb11f5b05a3d8f09f759f
    check("cr3-eos-r5-canon-eos-r5-craw-iso-100-nocrop-nodual.cr3", 0xdb64_0502_1fd7_ba92, (8352, 5586), (8192, 5464), true);
}

#[test]
fn canon_r50_craw_qp168_electronic_shutter_is_exact() {
    // raw.pixls.us/data/Canon/EOS%20R50/Canon_R50_ISO100_cRAW_electronicShutter.CR3; SHA256
    // 38fc458f2f74d0cfa04afd0ce513b40797080079f0786bedbfc55c39cc28d7d8
    check("cr3-eos-r50-canon-r50-iso100-craw-electronicshutter.cr3", 0xde7b_a402_5385_4e75, (6192, 4060), (6000, 4000), true);
}

#[test]
fn canon_r50_craw_qp168_mechanical_shutter_is_exact() {
    // raw.pixls.us/data/Canon/EOS%20R50/Canon_R50_ISO100_cRAW_mechanicShutter.CR3; SHA256
    // b79af9ffa184dd740e8af7a9f451f7150053d7e456b3435ca2e43bbbf5f65026
    check("cr3-eos-r50-canon-r50-iso100-craw-mechanicshutter.cr3", 0x0354_0075_5fe5_a2a7, (6192, 4060), (6000, 4000), true);
}

#[test]
fn canon_r6_craw_crop_odd_qp_map_is_exact() {
    // raw.pixls.us/data/Canon/EOS%20R6/Canon_EOS_R6_CRAW_ISO_100_crop_nodual.CR3; SHA256
    // 74abb0a113d075ad9887a058082f40dd2a938c4813a08474d82356f11a027778
    check("cr3-eos-r6-canon-eos-r6-craw-iso-100-crop-nodual.cr3", 0xb1c4_a2ee_f478_8db3, (3584, 2386), (3408, 2272), true);
}

#[test]
fn canon_r6_craw_odd_qp_map_is_exact() {
    // raw.pixls.us/data/Canon/EOS%20R6/Canon_EOS_R6_CRAW_ISO_100_nocrop_nodual.CR3; SHA256
    // 4775df76433b1e5844bc7a42b95d93efc8438c0a4cd9681e120e23fe3e3007e8
    check("cr3-eos-r6-canon-eos-r6-craw-iso-100-nocrop-nodual.cr3", 0xd3e0_567e_1ffb_07f4, (5568, 3708), (5472, 3648), true);
}

#[test]
fn canon_r6_mark_ii_craw_dual_crop_odd_qp_map_is_exact() {
    // raw.pixls.us/data/Canon/EOS%20R6%20Mark%20II/canon-r6m2-img_0356-craw-dual-crop.cr3; SHA256
    // deed19ea7b5178c70f64710c97bcdc9d21f6610c19b42a2310223e51034d4fc8
    check("cr3-eos-r6-mark-ii-canon-r6m2-img-0356-craw-dual-crop.cr3", 0xbe43_d848_a46f_60f5, (3936, 2612), (3744, 2496), true);
}

#[test]
fn canon_r6_mark_ii_craw_normal_crop_odd_qp_map_is_exact() {
    // raw.pixls.us/data/Canon/EOS%20R6%20Mark%20II/canon-r6m2-img_0359-craw-normal-crop.cr3; SHA256
    // 51ae1e673d2801d8a54f33dd8669c43034c506e2d355e75b26c0648e2f14a431
    check("cr3-eos-r6-mark-ii-canon-r6m2-img-0359-craw-normal-crop.cr3", 0xf491_c769_d297_ec65, (3936, 2612), (3744, 2496), true);
}

#[test]
fn canon_r7_craw_443a0159_odd_qp_map_is_exact() {
    // raw.pixls.us/data/Canon/EOS%20R7/443A0159.CR3; SHA256
    // 10a18e3f01ca9cd93408496d3853388ef140830548bfeed4237e40f967eb9d5c
    check("cr3-eos-r7-443a0159.cr3", 0xe972_065c_5929_4bf1, (7128, 4732), (6960, 4640), true);
}

#[test]
fn canon_r7_craw_443a0161_odd_qp_map_is_exact() {
    // raw.pixls.us/data/Canon/EOS%20R7/443A0161.CR3; SHA256
    // c01cee1b598120668370892c16a73da915f2aaec8169c5c60a96efd98d9aa220
    check("cr3-eos-r7-443a0161.cr3", 0xc6e0_dd45_1046_c484, (7128, 4732), (6960, 4640), true);
}

#[test]
fn canon_m50_craw_crop_is_the_shot_aspect() {
    // raw.pixls.us/getfile.php/2663: the 1:1 square starts 1000 px right of the recommended crop's origin,
    // which is (12, 12) inside the valid sensor area.
    let path = corpus().join("raw").join("cr3-canon-m50-craw.cr3");
    let Ok(bytes) = std::fs::read(&path) else {
        eprintln!("skip: {} absent", path.display());
        return;
    };
    let raw = decode(&bytes).unwrap();
    assert_eq!((raw.crop.x, raw.crop.y, raw.crop.width, raw.crop.height), (1012, 12, 4000, 4000));
}
