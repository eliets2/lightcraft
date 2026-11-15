//! Perf harness for the CRX entropy-decoder work: measures `lightcraft_raw::decode`
//! only (no pipeline), and prints FNV-1a sensor hashes for the whole CR3 corpus so a
//! base build and an optimized build can be compared byte-for-byte.
//!
//! Usage (release build mandatory for timings):
//!   cargo build --release -p lightcraft-raw --example crx_bench
//!   crx_bench sweep                     # "name WxH hash" or "name ERR <reason>" per corpus CR3
//!   crx_bench bench [reps] [warmups]    # decode-only ms + MP/s for 5 representative files

use lightcraft_raw::{RawData, decode};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

fn corpus() -> PathBuf {
    std::env::var_os("LIGHTCRAFT_CORPUS").map(PathBuf::from).unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/raw"))
}

fn fnv_sensor(bytes: &[u8]) -> Option<(usize, usize, u64)> {
    let raw = decode(bytes).ok()?;
    let RawData::U16(samples) = &raw.data else { return None };
    let hash = samples.iter().flat_map(|v| v.to_le_bytes()).fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3));
    Some((raw.width, raw.height, hash))
}

fn sweep() {
    let dir = corpus();
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("corpus dir")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("cr3")))
        .collect();
    paths.sort();
    let next = AtomicUsize::new(0);
    let lines = std::sync::Mutex::new(vec![String::new(); paths.len()]);
    std::thread::scope(|scope| {
        for _ in 0..6 {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::SeqCst);
                    if i >= paths.len() {
                        break;
                    }
                    let p = &paths[i];
                    let name = p.file_name().unwrap().to_string_lossy().to_string();
                    let bytes = std::fs::read(p).unwrap();
                    let line = match fnv_sensor(&bytes) {
                        Some((w, h, hash)) => format!("{name} {w}x{h} {hash:016x}"),
                        None => match decode(&bytes) {
                            Err(e) => format!("{name} ERR {e}"),
                            Ok(_) => format!("{name} ERR non-u16 data"),
                        },
                    };
                    println!("{line}");
                    lines.lock().unwrap()[i] = line;
                }
            });
        }
    });
    println!("=== BEGIN ===");
    for line in lines.into_inner().unwrap() {
        println!("{line}");
    }
}

const BENCH_FILES: &[(&str, &str)] = &[
    ("cr3-canon-r100-raw.cr3", "R100 lossless 0x100 (slow tail)"),
    ("cr3-canon-r8-craw.cr3", "R8 C-RAW 0x200 adaptive"),
    ("cr3-canon-m50-raw.cr3", "M50 lossless 0x100"),
    ("cr3-canon-eos-r5m2-raw.cr3", "R5m2 RAW 0x200"),
    ("cr3-eos-r5-canon-eos-r5-craw-iso-100-nocrop-dual.cr3", "R5 C-RAW dual-pixel"),
];

fn bench(reps: usize, warmups: usize) {
    for (name, label) in BENCH_FILES {
        let path = corpus().join(name);
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let first = decode(&bytes).expect(name);
        let mp = (first.width * first.height) as f64 / 1e6;
        for _ in 0..warmups {
            decode(&bytes).unwrap();
        }
        let mut times = Vec::new();
        for _ in 0..reps {
            let t = Instant::now();
            let img = decode(&bytes).unwrap();
            times.push(t.elapsed().as_secs_f64() * 1e3);
            assert_eq!((img.width, img.height), (first.width, first.height));
        }
        times.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let best = times[0];
        let median = times[times.len() / 2];
        println!(
            "{name:56} {label:30} {mp:6.1} MP  median {median:9.1} ms  best {best:9.1} ms  median {len:6.0} MP/s  best {best_mps:6.0} MP/s",
            len = mp / (median / 1e3),
            best_mps = mp / (best / 1e3),
        );
    }
}

fn config() {
    let dir = corpus();
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("corpus dir")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("cr3")))
        .collect();
    paths.sort();
    for p in paths {
        let bytes = std::fs::read(&p).unwrap();
        let Some(container) = lightcraft_meta::cr3::parse_cr3(&bytes) else { continue };
        for track in &container.tracks {
            let lightcraft_meta::cr3::Cr3TrackKind::Raw { cmp1: Some((at, length)), .. } = track.kind else { continue };
            let Some(Some(coding)) = bytes.get(at..at + length).map(lightcraft_meta::cr3::Cr3Compression::parse) else { continue };
            println!(
                "{} {:#x} levels {} {}x{} tile {}x{} tiles {}",
                p.file_name().unwrap().to_string_lossy(),
                coding.version,
                coding.levels,
                coding.width,
                coding.height,
                coding.tile_width,
                coding.tile_height,
                (coding.width as usize).div_ceil(coding.tile_width as usize) * (coding.height as usize).div_ceil(coding.tile_height as usize),
            );
        }
    }
}

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("sweep") => sweep(),
        Some("bench") => bench(
            std::env::args().nth(2).and_then(|n| n.parse().ok()).unwrap_or(3),
            std::env::args().nth(3).and_then(|n| n.parse().ok()).unwrap_or(1),
        ),
        Some("config") => config(),
        _ => eprintln!("usage: crx_bench (sweep | bench [reps] [warmups] | config)"),
    }
}
