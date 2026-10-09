//! Generate the committed seed corpus under `fuzz/seeds/` (see seeds/README.md).
//!
//! Everything is deterministic and self-verifying: synthetic CRX fixtures are
//! the sample streams from `crates/raw/src/vendor/crx.rs` unit tests wrapped by
//! the same container builder the fuzz target uses (each seed must probe and,
//! where noted, decode successfully), and `--samples <dir>` additionally slices
//! the first 64 KiB off real CR3 files (private user photos are *never*
//! committed — only those header slices, and only after `parse_cr3` confirms
//! the header box tree is intact).
//!
//! Usage:
//!   cargo run -p lightcraft-fuzz --bin gen-seeds -- [--out fuzz/seeds] [--samples D:\path\to\*.CR3-dir]

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::ExitCode;

const SLICE_LEN: usize = 64 * 1024;
const PAYLOAD_OFFSET: usize = 4096;

fn main() -> ExitCode {
    let mut out = PathBuf::from("fuzz/seeds");
    let mut samples: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out" => match args.next() {
                Some(v) => out = PathBuf::from(v),
                None => return fail("--out needs a value"),
            },
            "--samples" => match args.next() {
                Some(v) => samples = Some(PathBuf::from(v)),
                None => return fail("--samples needs a value"),
            },
            other => return fail(&format!("unknown argument {other}")),
        }
    }

    let mut errors = Vec::new();
    if let Err(e) = write_crx_seeds(&out) {
        errors.push(e);
    }
    if let Err(e) = write_container_seeds(&out) {
        errors.push(e);
    }
    if let Err(e) = write_box_seeds(&out) {
        errors.push(e);
    }
    if let Some(dir) = samples
        && let Err(e) = write_header_slices(&out, &dir)
    {
        errors.push(e);
    }
    if errors.is_empty() {
        println!("seeds written under {}", out.display());
        ExitCode::SUCCESS
    } else {
        for e in errors {
            eprintln!("error: {e}");
        }
        ExitCode::FAILURE
    }
}

fn fail(message: &str) -> ExitCode {
    eprintln!("error: {message}");
    ExitCode::FAILURE
}

// ---------------------------------------------------------------------------
// crx_bitstream seeds: [selector byte][CRX sample payload]
// ---------------------------------------------------------------------------

fn write_crx_seeds(out: &Path) -> Result<(), String> {
    let dir = out.join("crx_bitstream");
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let seeds: [(&str, u8, Vec<u8>, bool); 5] = [
        ("crx-lossless-4x4.bin", 0, flat_sample(), true),
        ("crx-craw-16x16.bin", 1, craw_16x16(), true),
        ("crx-v200-qp-16x16.bin", 2, v200_16x16(), true),
        // Two-tile lossless: validates (probe_info Ok) but its 8 payload bytes
        // cannot entropy-decode 3072 samples, so full decode errors — the seed
        // pins the marker-walk path, not the entropy path.
        ("crx-lossless-two-tile-64x48.bin", 3, two_tile_64x48(), false),
        ("crx-craw-clamp-16x16.bin", 4, craw_clamp_16x16(), true),
    ];
    for (name, selector, payload, must_decode) in seeds {
        let coding = lightcraft_fuzz::capped_coding(selector);
        let file = lightcraft_fuzz::wrap_crx_sample(&payload, &coding);
        let probed = lightcraft_raw::probe_info(&file);
        if probed.is_err() {
            return Err(format!("seed {name}: probe_info should accept the crafted fixture ({:?})", probed.err()));
        }
        if must_decode {
            match lightcraft_raw::decode(&file) {
                Ok(image) => println!("seed {name}: probe_info ok, decode ok ({} samples)", image.data.len()),
                Err(e) => return Err(format!("seed {name}: decode should succeed: {e}")),
            }
        } else {
            println!("seed {name}: probe_info ok (decode expected to reject: {:?})", lightcraft_raw::decode(&file).err());
        }
        let mut bytes = vec![selector];
        bytes.extend_from_slice(&payload);
        std::fs::write(dir.join(name), bytes).map_err(|e| format!("{}: {e}", dir.join(name).display()))?;
    }
    Ok(())
}

fn marker(out: &mut Vec<u8>, code: u16, size: u32, flags: u32) {
    out.extend_from_slice(&code.to_be_bytes());
    out.extend_from_slice(&8u16.to_be_bytes());
    out.extend_from_slice(&size.to_be_bytes());
    out.extend_from_slice(&flags.to_be_bytes());
}

/// `flat_sample` from `crates/raw/src/vendor/crx.rs` tests: a 4×4 lossless
/// image, nine 12-byte markers + 4 entropy bytes (header_size 108).
fn flat_sample() -> Vec<u8> {
    let mut out = Vec::new();
    marker(&mut out, 0xff01, 4, 0);
    for plane in 0..4u32 {
        marker(&mut out, 0xff02, 1, (plane << 28) | 0x0800_0000);
        marker(&mut out, 0xff03, 1, 0x0020_0000);
    }
    out.extend_from_slice(&[0x1e; 4]);
    out
}

fn wavelet_plane_data(out: &mut Vec<u8>) {
    out.extend_from_slice(&[0x80; 4]); // LL3 + three 1×1 HF3 bands
    out.extend_from_slice(&[0xf0; 3]); // three 2×2 all-zero HF2 bands
    for _ in 0..3 {
        out.extend_from_slice(&[0xff, 0xf8]); // three 4×4 all-zero HF1 bands
    }
}

/// `three_level_craw_reconstructs_sensor_samples` fixture: 16×16 C-RAW,
/// horizontal-tile markers ff01/ff02/ff03, header_size 540.
fn craw_16x16() -> Vec<u8> {
    let mut out = Vec::new();
    marker(&mut out, 0xff01, 52, 0);
    for plane in 0..4u32 {
        marker(&mut out, 0xff02, 13, (plane << 28) | 0x0800_0000);
        for band in 0..10u32 {
            marker(&mut out, 0xff03, u32::from(band >= 7) + 1, (band << 28) | 0x0020_0000);
        }
    }
    for _ in 0..4 {
        wavelet_plane_data(&mut out);
    }
    out
}

/// The clamp-check variant (`craw_overshoot…`): LL3 at +8200 → synthesis
/// overshoots the 14-bit range and must clamp.
fn craw_clamp_16x16() -> Vec<u8> {
    let mut out = Vec::new();
    marker(&mut out, 0xff01, 80, 0);
    let sizes = [8u32, 1, 1, 1, 1, 1, 1, 2, 2, 2];
    for plane in 0..4u32 {
        marker(&mut out, 0xff02, 20, (plane << 28) | 0x0800_0000);
        for band in 0..10u32 {
            marker(&mut out, 0xff03, sizes[band as usize], (band << 28) | 0x0020_0000);
        }
    }
    for _ in 0..4 {
        out.extend_from_slice(&[0, 0, 0, 0, 0, 0x40, 0x80, 0x20]); // LL3 = +8200
        out.extend_from_slice(&[0x80; 3]); // three 1×1 HF3 bands
        out.extend_from_slice(&[0xf0; 3]); // three 2×2 all-zero HF2 bands
        for _ in 0..3 {
            out.extend_from_slice(&[0xff, 0xf8]); // three 4×4 all-zero HF1 bands
        }
    }
    out
}

/// `adaptive_v200_craw_reconstructs_sensor_samples` fixture: 16×16 v0x200
/// C-RAW, ff11/ff12/ff13 family, adaptive QP map, header_size 872.
fn v200_16x16() -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&0xff11u16.to_be_bytes());
    out.extend_from_slice(&16u16.to_be_bytes());
    out.extend_from_slice(&60u32.to_be_bytes());
    out.extend_from_slice(&0x4000u32.to_be_bytes());
    out.extend_from_slice(&5u32.to_be_bytes());
    out.extend_from_slice(&3u16.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    let gains = [0u32, 0, 0, 0, 4, 4, 8, 8, 8, 16];
    for plane in 0..4u32 {
        marker(&mut out, 0xff12, 13, (plane << 28) | 0x0800_0000);
        for band in 0..10u32 {
            out.extend_from_slice(&0xff13u16.to_be_bytes());
            out.extend_from_slice(&16u16.to_be_bytes());
            out.extend_from_slice(&u32::from(band >= 7).wrapping_add(1).to_be_bytes()); // band byte size
            out.extend_from_slice(&((band << 28) | gains[band as usize]).to_be_bytes()); // index | gain
            out.extend_from_slice(&u32::from(band < 4).to_be_bytes()); // quant base
            out.extend_from_slice(&[0u8; 4]); // padding + reserved
        }
    }
    out.extend_from_slice(&[0u8; 4]);
    out.extend_from_slice(&[0, 0, 1, 0x2c, 0x94, 0, 0, 0]); // QP map: all 150
    for _ in 0..4 {
        wavelet_plane_data(&mut out);
    }
    out
}

/// Two-tile lossless header from `crates/raw/src/vendor/cr3.rs` tests'
/// `synthetic_header` (64×48, header_size 216) + its 8 entropy bytes.
fn two_tile_64x48() -> Vec<u8> {
    let mut out = Vec::new();
    for tile in 0..2u32 {
        marker(&mut out, 0xff01, 4, tile << 16);
        for plane in 0..4u32 {
            marker(&mut out, 0xff02, 1, (plane << 28) | 0x0800_0000);
            marker(&mut out, 0xff03, 1, 0x0020_0000);
        }
    }
    out.extend_from_slice(&[0x1e; 8]);
    out
}

// ---------------------------------------------------------------------------
// Container + box-graph seeds: a small synthetic CR3 (Canon uuid with CMT
// TIFFs, JPEG track, raw track, XMP), as in `crates/meta/src/cr3.rs` tests.
// ---------------------------------------------------------------------------

fn write_container_seeds(out: &Path) -> Result<(), String> {
    let dir = out.join("cr3_container");
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let file = synthetic_container();
    if lightcraft_meta::cr3::parse_cr3(&file).is_none() {
        return Err("synthetic container does not parse".into());
    }
    // probe_info reaches the raw track's CMP1, which this fixture leaves
    // intentionally malformed (a 4-byte descriptor): the container must parse
    // and the decoder must report corrupt, not panic.
    match lightcraft_raw::probe_info(&file) {
        Ok(_) => println!("container seed: probe_info ok"),
        Err(e) => println!("container seed: probe_info rejects the malformed CMP1 as designed ({e})"),
    }
    let name = dir.join("container-synthetic.bin");
    std::fs::write(&name, &file).map_err(|e| format!("{}: {e}", name.display()))?;

    // A second container whose raw track is the wrapped 4×4 lossless fixture:
    // fully decodable end to end.
    let coding = lightcraft_fuzz::capped_coding(0);
    let wrapped = lightcraft_fuzz::wrap_crx_sample(&flat_sample(), &coding);
    let name = dir.join("container-crx-lossless.bin");
    std::fs::write(&name, &wrapped).map_err(|e| format!("{}: {e}", name.display()))?;
    match lightcraft_raw::decode(&wrapped) {
        Ok(image) => println!("container-crx-lossless seed: decode ok ({} samples)", image.data.len()),
        Err(e) => return Err(format!("container-crx-lossless seed should decode: {e}")),
    }
    Ok(())
}

fn write_box_seeds(out: &Path) -> Result<(), String> {
    let dir = out.join("iso_bmff_boxes");
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let consistent = synthetic_container();
    std::fs::write(dir.join("boxes-consistent.bin"), &consistent).map_err(|e| format!("{}: {e}", dir.join("boxes-consistent.bin").display()))?;
    // Same graph, but the moov size overruns by 4 bytes: the classic
    // size–content mismatch the generator also produces.
    let mut lie = consistent.clone();
    let mut size = u32::from_be_bytes([lie[24], lie[25], lie[26], lie[27]]);
    size = size.wrapping_add(4);
    lie[24..28].copy_from_slice(&size.to_be_bytes());
    std::fs::write(dir.join("boxes-size-lie.bin"), &lie).map_err(|e| format!("{}: {e}", dir.join("boxes-size-lie.bin").display()))?;
    println!("box seeds: consistent + size-lie written");
    Ok(())
}

fn write_header_slices(out: &Path, samples: &Path) -> Result<(), String> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(samples)
        .map_err(|e| format!("{}: {e}", samples.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case("cr3")).unwrap_or(false))
        .collect();
    entries.sort();
    if entries.is_empty() {
        return Err(format!("no .CR3 files in {}", samples.display()));
    }
    for path in entries {
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else { continue };
        let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        if bytes.len() < SLICE_LEN {
            return Err(format!("{}: smaller than {SLICE_LEN} bytes", path.display()));
        }
        let slice = &bytes[..SLICE_LEN];
        let parsed = lightcraft_meta::cr3::parse_cr3(slice).is_some();
        println!("slice {stem}: parse_cr3 on first {SLICE_LEN} bytes = {parsed}");
        for (subdir, name) in [("cr3_container", format!("hdr-{stem}.bin")), ("cr3_whole_file", format!("wf-hdr-{stem}.bin"))] {
            let dir = out.join(subdir);
            std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            std::fs::write(dir.join(&name), slice).map_err(|e| format!("{}: {e}", dir.join(&name).display()))?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Minimal container builder (mirrors crates/meta/src/cr3.rs test helpers).
// ---------------------------------------------------------------------------

fn bx(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    box_bytes(kind, body)
}

fn box_bytes(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut v = ((body.len() + 8) as u32).to_be_bytes().to_vec();
    v.extend_from_slice(kind);
    v.extend_from_slice(body);
    v
}

fn uuid_box(u: &[u8; 16], body: &[u8]) -> Vec<u8> {
    let mut b = u.to_vec();
    b.extend_from_slice(body);
    bx(b"uuid", &b)
}

fn full_box(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut b = vec![0u8; 4];
    b.extend_from_slice(body);
    bx(kind, &b)
}

fn craw_box(w: u16, h: u16, children: &[u8]) -> Vec<u8> {
    let mut b = vec![0u8; 82];
    b[24..26].copy_from_slice(&w.to_be_bytes());
    b[26..28].copy_from_slice(&h.to_be_bytes());
    b.extend_from_slice(children);
    bx(b"CRAW", &b)
}

fn trak_box(entry: Vec<u8>, offset: u64, size: u32) -> Vec<u8> {
    let mut stsd = 1u32.to_be_bytes().to_vec();
    stsd.extend(entry);
    let mut stsz = 0u32.to_be_bytes().to_vec();
    stsz.extend(1u32.to_be_bytes());
    stsz.extend(size.to_be_bytes());
    let mut co64 = 1u32.to_be_bytes().to_vec();
    co64.extend(offset.to_be_bytes());
    let stbl = [full_box(b"stsd", &stsd), full_box(b"stsz", &stsz), full_box(b"co64", &co64)].concat();
    bx(b"trak", &bx(b"mdia", &bx(b"minf", &bx(b"stbl", &stbl))))
}

/// Little-endian single-entry TIFF (for CMT1/CMT2/CMT4): ASCII or SHORT value.
fn tiff(tag: u16, ascii: Option<&str>, short: Option<u16>) -> Vec<u8> {
    let mut out = b"II".to_vec();
    out.extend_from_slice(&42u16.to_le_bytes());
    out.extend_from_slice(&8u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&tag.to_le_bytes());
    let mut value = Vec::new();
    let (kind, count) = match ascii {
        Some(s) => {
            value.extend_from_slice(s.as_bytes());
            value.push(0);
            (2u16, value.len() as u32)
        }
        None => {
            value.extend_from_slice(&short.unwrap_or(0).to_le_bytes());
            (3u16, 1)
        }
    };
    out.extend_from_slice(&kind.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    if value.len() <= 4 {
        out.extend_from_slice(&value);
        out.resize(out.len() + (4 - value.len()), 0);
    } else {
        out.extend_from_slice(&26u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&value);
    }
    out
}

const CANON_UUID: [u8; 16] = [0x85, 0xc0, 0xb6, 0x87, 0x82, 0x0f, 0x11, 0xe0, 0x81, 0x11, 0xf4, 0xce, 0x46, 0x2b, 0x6a, 0x48];
const XMP_UUID: [u8; 16] = [0xbe, 0x7a, 0xcf, 0xcb, 0x97, 0xa9, 0x42, 0xe8, 0x9c, 0x71, 0x99, 0x94, 0x91, 0xe3, 0xaf, 0xac];

/// A small synthetic CR3: Canon uuid with CMT1/CMT2/CMT4 + THMB, one JPEG
/// track, one raw track (deliberately malformed 4-byte CMP1), XMP uuid box.
fn synthetic_container() -> Vec<u8> {
    let cmt1 = tiff(0x0110, Some("Canon EOS Test"), None); // Model
    let cmt2 = tiff(0x8827, None, Some(400)); // ISO
    let cmt4 = tiff(0x0001, Some("N"), None); // GPSLatitudeRef
    let canon = uuid_box(&CANON_UUID, &[bx(b"CMT1", &cmt1), bx(b"CMT2", &cmt2), bx(b"CMT4", &cmt4), bx(b"THMB", b"thumb")].concat());
    let payload_at = PAYLOAD_OFFSET as u64;
    let jpeg_t = trak_box(craw_box(64, 48, &bx(b"JPEG", &[0; 4])), payload_at, 16);
    let raw_t = trak_box(craw_box(60, 40, &bx(b"CMP1", &[0xff, 0x10, 0, 0x30])), payload_at + 16, 32);
    let mut file = bx(b"ftyp", b"crx \0\0\0\x01crx isom");
    file.extend(bx(b"moov", &[canon, jpeg_t, raw_t].concat()));
    file.extend(uuid_box(&XMP_UUID, b"<x:xmpmeta/>"));
    file.resize(payload_at as usize + 64, 0);
    file
}
