//! Fuzz-target bodies for LightCraft's CR3 container and CRX bitstream parsers.
//!
//! Layout: the fuzz_targets/*.rs binaries are thin libFuzzer shims (a plain
//! `#[no_mangle] pub extern "C" fn LLVMFuzzerTestOneInput`) over the functions
//! here; `src/bin/mutate-smoke.rs` calls the very same functions, so the
//! deterministic Windows smoke harness exercises byte-for-byte the code that
//! Linux CI fuzzes. Every body must itself be panic-free — any panic that
//! escapes is by definition a finding in the code under test, never in the
//! harness.
//!
//! Guards mirrored from the verification plan (research/E-corpus-verification.md §3.1):
//! inputs are capped per target (container 1 MiB, bitstream 8 MiB), the CRX
//! harness pins CMP1 geometry to tiny values so a single decode is bounded,
//! and the box-graph generator bounds depth/size so generation cannot explode.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use lightcraft_meta::cr3::Cr3Compression;

/// Per-target input caps (bytes). Container targets take whole files, so their
/// cap is the plan's `-max_len` for `cr3_container`/`iso_bmff_boxes`.
pub const MAX_CONTAINER_BYTES: usize = 1 << 20;
/// The plan's `-max_len` for `crx_bitstream` (payload-only input).
pub const MAX_BITSTREAM_BYTES: usize = 8 << 20;
/// Whole-file develop is capped to this many sensor samples (24 MP): bounds the
/// demosaic's memory (~400 MB peak) under the libFuzzer `-rss_limit_mb=2048` guard.
pub const MAX_DEVELOP_SAMPLES: usize = 24_000_000;

const PAYLOAD_OFFSET: usize = 4096;

/// The four targets, for dispatch by name from the smoke harness.
pub const TARGETS: [&str; 4] = ["cr3_container", "crx_bitstream", "cr3_whole_file", "iso_bmff_boxes"];

/// Run one target by name (the smoke harness's dispatch).
pub fn dispatch(target: &str, data: &[u8]) {
    match target {
        "cr3_container" => cr3_container(data),
        "crx_bitstream" => crx_bitstream(data),
        "cr3_whole_file" => cr3_whole_file(data),
        "iso_bmff_boxes" => iso_bmff_boxes(data),
        _ => {}
    }
}

/// `cr3_container`: the ISO-BMFF container layer on raw bytes — box walk, TIFF
/// descriptor parsers, merged Exif, timed metadata, probe/probe_info (the CRX
/// header walk) and embedded-preview extraction. Must never panic or loop
/// unbounded; never allocate for sizes the input does not actually hold.
pub fn cr3_container(data: &[u8]) {
    container_sink(&data[..data.len().min(MAX_CONTAINER_BYTES)]);
}

/// `crx_bitstream`: structure-aware CRX fuzzing. The input's first byte selects
/// a CMP1 descriptor with *capped* geometry (at most 64×48, so one decode is
/// milliseconds at worst); the remainder replaces the CRX sample payload inside
/// a synthetic-but-valid container. Exercises the full marker walk (`validate`)
/// and, when the crafted header is consistent, the entropy decoder, dequantizer
/// and 5/3 wavelet (`decode`).
pub fn crx_bitstream(data: &[u8]) {
    let data = &data[..data.len().min(MAX_BITSTREAM_BYTES)];
    let Some((&selector, payload)) = data.split_first() else { return };
    let coding = capped_coding(selector);
    let file = wrap_crx_sample(payload, &coding);
    let _ = lightcraft_raw::probe(&file);
    let _ = lightcraft_raw::probe_info(&file);
    let _ = lightcraft_raw::decode(&file);
}

/// `cr3_whole_file`: byte-mutated real CR3s through the whole pipeline the
/// engine runs: container parse → probe → probe_info → preview extraction →
/// decode → develop (binned preview path, then full develop for small images).
/// Seeds are real files (kept out of the repository; see seeds/README.md).
pub fn cr3_whole_file(data: &[u8]) {
    let _ = lightcraft_raw::probe(data);
    if let Some(container) = lightcraft_meta::cr3::parse_cr3(data) {
        let _ = lightcraft_meta::cr3::merged_exif(&container);
    }
    let _ = lightcraft_raw::probe_info(data);
    let _ = lightcraft_raw::embedded_preview(data);
    if let Ok(image) = lightcraft_raw::decode(data) {
        // The import/preview path bins 1/4 straight from the mosaic; fall back
        // to a full develop only when the image is small enough to bound a
        // single case's CPU and memory.
        let binned = image.develop_binned(4, 1.0).unwrap_or(None);
        if binned.is_none() && image.width.saturating_mul(image.height) <= MAX_DEVELOP_SAMPLES {
            let _ = image.develop(lightcraft_raw::Method::Bilinear);
        }
    }
}

/// `iso_bmff_boxes`: structure-aware generator. `arbitrary` turns raw fuzzer
/// bytes into a box graph built from the known CR3 fourcc pool with consistent
/// sizes, plus occasional size–content mismatches; the result runs through the
/// same sink as `cr3_container`.
pub fn iso_bmff_boxes(data: &[u8]) {
    let mut u = arbitrary::Unstructured::new(&data[..data.len().min(MAX_CONTAINER_BYTES)]);
    if let Ok(file) = arbitrary_box_graph(&mut u) {
        container_sink(&file);
    }
}

/// Everything the container targets observe, in one place.
fn container_sink(data: &[u8]) {
    // Unit-level: the two descriptor parsers take raw payloads directly.
    let _ = lightcraft_meta::cr3::Cr3Compression::parse(data);
    let _ = lightcraft_meta::cr3::Cr3ImageArea::parse(data);
    if let Some(container) = lightcraft_meta::cr3::parse_cr3(data) {
        let _ = lightcraft_meta::cr3::merged_exif(&container);
        let _ = container.timed_exif(data);
        for track in &container.tracks {
            let _ = track.compression(data);
            let _ = track.image_area(data);
        }
    }
    let _ = lightcraft_raw::probe(data);
    if let Ok(info) = lightcraft_raw::probe_info(data) {
        let _ = info.developed_size();
    }
    let _ = lightcraft_raw::embedded_preview(data);
}

/// CMP1 descriptors for the structure-aware CRX target, keyed by the input's
/// first byte. Every geometry stays tiny (worst case 64×48 sensor, ≤ 3072
/// samples), so one full decode is bounded work; `header_size` matches the
/// fixture marker layouts from `crates/raw/src/vendor/crx.rs` unit tests, so
/// the committed seeds are valid streams that reach the entropy decoder.
pub fn capped_coding(selector: u8) -> Cr3Compression {
    let (version, levels, width, height, tile_width, tile_height, header_size) = match selector % 5 {
        0 => (0x100u16, 0u8, 4u32, 4u32, 4u32, 4u32, 108u32),
        1 => (0x100, 3, 16, 16, 16, 16, 540),
        2 => (0x200, 3, 16, 16, 16, 16, 872),
        3 => (0x100, 0, 64, 48, 32, 48, 216),
        _ => (0x100, 3, 16, 16, 16, 16, 540),
    };
    Cr3Compression {
        version,
        width,
        height,
        tile_width,
        tile_height,
        bit_depth: 14,
        planes: 4,
        cfa_pattern: (u32::from(selector) / 5 % 4) as u8,
        encoding: 0,
        levels,
        tile_flags: 0,
        header_size,
        median_bit_depth: None,
    }
}

/// Wrap a CRX sample payload in a minimal, structurally valid CR3 container:
/// `ftyp` + `moov`(trak(mdia(minf(stbl{stsd(CRAW{CMP1})}, stsz, stco)))) with
/// the sample at a fixed 4096-byte offset. Mirrors the fixture construction in
/// `crates/raw/src/vendor/cr3.rs` tests and `crates/meta/src/cr3.rs` tests.
pub fn wrap_crx_sample(sample: &[u8], coding: &Cr3Compression) -> Vec<u8> {
    let mut file = box_bytes(b"ftyp", b"crx \0\0\0\x01crx isom");
    let craw = craw_entry(coding);
    let mut stsd_body = [0u32.to_be_bytes().as_slice(), 1u32.to_be_bytes().as_slice()].concat();
    stsd_body.extend_from_slice(&craw); // the CRAW sample entry lives inside stsd
    let stsd = box_bytes(b"stsd", &stsd_body);
    let count = 1u32.to_be_bytes();
    let mut stsz_body = [0u32.to_be_bytes().as_slice(), 0u32.to_be_bytes().as_slice(), count.as_slice()].concat();
    stsz_body.extend_from_slice(&u32::try_from(sample.len()).unwrap_or(u32::MAX).to_be_bytes());
    let stsz = box_bytes(b"stsz", &stsz_body);
    let mut stco_body = [0u32.to_be_bytes().as_slice(), count.as_slice()].concat();
    stco_body.extend_from_slice(&u32::try_from(PAYLOAD_OFFSET).unwrap_or(u32::MAX).to_be_bytes());
    let stco = box_bytes(b"stco", &stco_body);
    let stbl_body = [stsd, stsz, stco].concat();
    let stbl = box_bytes(b"stbl", &stbl_body);
    let minf = box_bytes(b"minf", &stbl);
    let mdia = box_bytes(b"mdia", &minf);
    let trak = box_bytes(b"trak", &mdia);
    file.extend(box_bytes(b"moov", &trak));
    let pad = PAYLOAD_OFFSET.saturating_sub(file.len());
    file.resize(file.len() + pad, 0);
    file.extend_from_slice(sample);
    file
}

/// A `CRAW` sample entry: 82 fixed bytes (dimensions at 24..28) + a `CMP1` box.
/// The entry's width/height must match the CMP1 sensor geometry — the container
/// picks the main track by largest area, so zeros would hide the raw track.
fn craw_entry(coding: &Cr3Compression) -> Vec<u8> {
    let mut body = vec![0u8; 82];
    let (width, height) = (u16::try_from(coding.width).unwrap_or(u16::MAX), u16::try_from(coding.height).unwrap_or(u16::MAX));
    body[24..26].copy_from_slice(&width.to_be_bytes());
    body[26..28].copy_from_slice(&height.to_be_bytes());
    let mut cmp1 = [0u8; 52];
    cmp1[0..2].copy_from_slice(&0xff00u16.to_be_bytes());
    cmp1[2..4].copy_from_slice(&48u16.to_be_bytes()); // header length word: 48 + 4 = 52
    cmp1[4..6].copy_from_slice(&coding.version.to_be_bytes());
    for (at, value) in [(8, coding.width), (12, coding.height), (16, coding.tile_width), (20, coding.tile_height), (28, coding.header_size)] {
        cmp1[at..at + 4].copy_from_slice(&value.to_be_bytes());
    }
    cmp1[24] = coding.bit_depth;
    cmp1[25] = (coding.planes << 4) | (coding.cfa_pattern & 15);
    cmp1[26] = (coding.encoding << 4) | (coding.levels & 15);
    cmp1[27] = coding.tile_flags;
    body.extend_from_slice(&box_bytes(b"CMP1", &cmp1));
    box_bytes(b"CRAW", &body)
}

fn box_bytes(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let size = u32::try_from(body.len().saturating_add(8)).unwrap_or(0);
    let mut out = Vec::with_capacity(body.len() + 8);
    out.extend_from_slice(&size.to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(body);
    out
}

/// Fourcc pool from the verification plan (research/E §3.1) plus the supporting
/// ISO-BMFF boxes the parser reads (stsd/stsz/stco/co64, JPEG/HEVC, free).
const BOX_KINDS: [&[u8; 4]; 28] = [
    b"moov", b"trak", b"mdia", b"minf", b"stbl", b"uuid", b"stsd", b"stsz", b"stco", b"co64", b"CRAW", b"CMP1", b"CDI1", b"IAD1", b"CTMD", b"CMT1",
    b"CMT2", b"CMT3", b"CMT4", b"THMB", b"CTBO", b"CCTP", b"CNCV", b"PRVW", b"mdat", b"JPEG", b"HEVC", b"free",
];
/// Boxes the generator may recurse into.
const PARENT_KINDS: [&[u8; 4]; 7] = [b"moov", b"trak", b"mdia", b"minf", b"stbl", b"uuid", b"CDI1"];

fn is_parent(kind: &[u8; 4]) -> bool {
    PARENT_KINDS.contains(&kind)
}

/// Generate a box graph from `u`. Starts with a real `ftypcrx ` header (so the
/// container parser proceeds past the magic), then nests known boxes with
/// consistent sizes; roughly one input byte in eight produces a size–content
/// mismatch (overshoot, undershoot, to-end `size=0` or 64-bit `size=1` without
/// the extended header) to exercise the walker's failure paths.
fn arbitrary_box_graph(u: &mut arbitrary::Unstructured) -> arbitrary::Result<Vec<u8>> {
    let mut file = Vec::with_capacity(4096);
    file.extend_from_slice(&24u32.to_be_bytes());
    file.extend_from_slice(b"ftypcrx \0\0\0\x01crx isom");
    let budget = u.int_in_range(0..=64 * 1024usize)?;
    let depth = u.int_in_range(0..=6usize)?;
    emit_boxes(u, &mut file, budget, depth)?;
    Ok(file)
}

fn emit_boxes(u: &mut arbitrary::Unstructured, out: &mut Vec<u8>, mut budget: usize, depth: usize) -> arbitrary::Result<()> {
    while budget > 24 {
        if u.arbitrary::<u8>()? % 8 == 0 {
            break;
        }
        let kind = *BOX_KINDS[usize::from(u.arbitrary::<u8>()? % BOX_KINDS.len() as u8)];
        let lie = u.arbitrary::<u8>()?;
        let mut body = Vec::new();
        if &kind == b"uuid" {
            body.extend_from_slice(&u.arbitrary::<[u8; 16]>()?);
        }
        if is_parent(&kind) && depth > 0 {
            emit_boxes(u, &mut body, budget / 2, depth - 1)?;
        } else {
            leaf_body(u, &kind, &mut body)?;
        }
        let consistent = u32::try_from(body.len().saturating_add(8)).unwrap_or(u32::MAX);
        let size = match lie % 16 {
            0..=11 => consistent,
            12 => consistent.saturating_add(u32::from(lie % 8)), // header overruns the next box
            13 => consistent.saturating_sub(u32::from(lie % 8)).max(1).min(consistent), // body truncated
            14 => 0,                                             // size extends to end of file
            _ => 1,                                              // 64-bit size announced but not present
        };
        budget = budget.saturating_sub(8usize).saturating_sub(body.len().min(budget));
        out.extend_from_slice(&size.to_be_bytes());
        out.extend_from_slice(&kind);
        out.extend_from_slice(&body);
        if out.len() > 256 * 1024 {
            break;
        }
    }
    Ok(())
}

/// Semi-structured leaves: table boxes get full-box headers and counts, `CRAW`
/// gets a VisualSampleEntry-shaped body, `CMP1`/`IAD1` plausible payload
/// lengths — so mutations often stay *inside* one parser instead of dying on
/// the first length check.
fn leaf_body(u: &mut arbitrary::Unstructured, kind: &[u8; 4], body: &mut Vec<u8>) -> arbitrary::Result<()> {
    match kind {
        b"stsd" | b"stsz" | b"stco" | b"co64" => {
            body.extend_from_slice(&u.arbitrary::<u32>()?.to_be_bytes());
            body.extend_from_slice(&u.arbitrary::<u32>()?.to_be_bytes());
            let n = usize::from(u.arbitrary::<u8>()? % 4);
            for _ in 0..n {
                let entry = u.arbitrary::<u32>()?;
                body.extend_from_slice(&entry.to_be_bytes());
            }
            // A stsd usually holds one sample entry; give it a CRAW child so
            // mutations land inside `sample_entry` instead of on an empty table.
            if kind == b"stsd" && u.arbitrary::<u8>()? % 2 == 0 {
                let (width, height) = (u.arbitrary::<u16>()?, u.arbitrary::<u16>()?);
                let cmp1_len = u.int_in_range(8..=64usize)?;
                let mut craw = vec![0u8; 82];
                craw[24..26].copy_from_slice(&width.to_be_bytes());
                craw[26..28].copy_from_slice(&height.to_be_bytes());
                craw.extend_from_slice(&box_bytes(b"CMP1", &random_bytes(u, cmp1_len)));
                body.extend_from_slice(&box_bytes(b"CRAW", &craw));
            }
        }
        b"CRAW" => {
            let (width, height) = (u.arbitrary::<u16>()?, u.arbitrary::<u16>()?);
            let cmp1_len = u.int_in_range(8..=64usize)?;
            body.extend_from_slice(&random_bytes(u, 82));
            body[24..26].copy_from_slice(&width.to_be_bytes());
            body[26..28].copy_from_slice(&height.to_be_bytes());
            body.extend_from_slice(&box_bytes(b"CMP1", &random_bytes(u, cmp1_len)));
        }
        b"CMP1" | b"IAD1" | b"CTBO" => {
            let n = u.int_in_range(8..=96usize)?;
            body.extend_from_slice(&random_bytes(u, n));
        }
        _ => {
            let n = usize::from(u.arbitrary::<u8>()? % 32);
            body.extend_from_slice(u.bytes(n)?);
        }
    }
    Ok(())
}

fn random_bytes(u: &mut arbitrary::Unstructured, n: usize) -> Vec<u8> {
    let mut out = u.bytes(n).map(<[u8]>::to_vec).unwrap_or_default();
    // The harness must never panic even when the fuzzer input is exhausted:
    // pad with a fixed pattern so every caller can slice what it asked for.
    out.resize(n, 0xA5);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The 4×4 lossless fixture from `crates/raw/src/vendor/crx.rs` tests:
    /// nine 12-byte markers + 4 bytes of entropy data, header_size 108.
    fn flat_sample() -> Vec<u8> {
        let mut out = Vec::new();
        let marker = |out: &mut Vec<u8>, code: u16, size: u32, flags: u32| {
            out.extend_from_slice(&code.to_be_bytes());
            out.extend_from_slice(&8u16.to_be_bytes());
            out.extend_from_slice(&size.to_be_bytes());
            out.extend_from_slice(&flags.to_be_bytes());
        };
        marker(&mut out, 0xff01, 4, 0);
        for plane in 0..4u32 {
            marker(&mut out, 0xff02, 1, (plane << 28) | 0x0800_0000);
            marker(&mut out, 0xff03, 1, 0x0020_0000);
        }
        out.extend_from_slice(&[0x1e; 4]);
        out
    }

    #[test]
    fn wrapped_lossless_fixture_decodes() {
        let coding = capped_coding(0);
        let file = wrap_crx_sample(&flat_sample(), &coding);
        assert!(lightcraft_meta::cr3::is_cr3(&file));
        let image = lightcraft_raw::decode(&file).unwrap();
        assert_eq!((image.width, image.height), (4, 4));
        assert_eq!(image.data.len(), 16);
    }

    #[test]
    fn every_truncation_of_the_wrapped_fixture_is_an_error_not_a_panic() {
        let file = wrap_crx_sample(&flat_sample(), &capped_coding(0));
        for n in 0..file.len() {
            cr3_container(&file[..n]);
            let _ = lightcraft_raw::decode(&file[..n]);
        }
    }

    #[test]
    fn box_graph_generator_is_bounded_and_deterministic() {
        let mut generated = 0;
        for seed in 0..64u8 {
            let mut input = vec![seed; 4096];
            input.extend(0u8..=255);
            let mut u = arbitrary::Unstructured::new(&input);
            // Exhausted input legitimately yields Err (targets skip it): only
            // successful generations must be bounded and deterministic.
            if let Ok(file) = arbitrary_box_graph(&mut u) {
                generated += 1;
                assert!(file.len() <= 256 * 1024 + 24);
                assert_eq!(arbitrary_box_graph(&mut arbitrary::Unstructured::new(&input)).unwrap(), file);
            }
        }
        assert!(generated > 8, "only {generated} of 64 inputs generated a graph");
    }

    #[test]
    fn target_bodies_smoke_over_the_box_generator() {
        for seed in 0..8u16 {
            let input = (0..512u32).map(|i| (i * u32::from(seed) % 251) as u8).collect::<Vec<_>>();
            dispatch("cr3_container", &input);
            dispatch("crx_bitstream", &input);
            dispatch("iso_bmff_boxes", &input);
            dispatch("cr3_whole_file", &input);
        }
    }
}
