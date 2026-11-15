//! Canon CR3 container, camera metadata and sensor geometry.
//!
//! Sources: Laurent Clévy's public CR3 format description (`CMP1`, `CDI1/IAD1`, `CMT3`),
//! <https://github.com/lclevy/canon_cr3/blob/master/readme.md>, and ExifTool's published Canon
//! tag-name tables (`SensorInfo`, `ColorData9`…`ColorData12`, `ColorCalib`, `LensModel`),
//! <https://exiftool.org/TagNames/Canon.html>. No third-party decoder source was consulted.
//!
//! `IAD1` stores the valid sensor area separately from the recommended image crop. Both use inclusive
//! sensor offsets. The CFA is anchored at the full sensor's origin; cropping shifts its phase later.
//! Unknown camera metadata versions are ignored rather than assigned a guessed white balance.
//!
//! Colour: the file's `ColorData` carries its own white-balance planes (a neutral surface's raw
//! levels for each stored illuminant, with the illuminant's Kelvin temperature). The XYZ → camera
//! matrices of the DNG colour model are fitted to those planes and validated against them, so a
//! wrong layout or a body whose planes don't fit keeps the embedded-JPEG colour fallback.

use super::{black_from_columns, crx, white_from_data};
use crate::{BlackLevel, Cfa, ColorData, MAX_SAMPLES, Mat3, Mode, OpcodeLists, RawData, RawError, RawFormat, RawImage, Rect, Result};
use lightcraft_color::{SRGB, Xy, cct};
use lightcraft_meta::cr3::{Cr3ImageArea, Cr3Track, Cr3TrackKind, parse_cr3};
use lightcraft_tiff::{Ifd, Tiff};

const SENSOR_INFO: u16 = 0x00e0;
const COLOR_DATA: u16 = 0x4001;
const LENS_MODEL: u16 = 0x0095;

/// Pick the first largest raw track. Equal-sized later tracks can be the Dual Pixel delta, which
/// must never replace the main sensor image. An invalid full-size descriptor is reported, not
/// silently replaced with the reduced raw preview.
fn main_track(tracks: &[Cr3Track]) -> Option<&Cr3Track> {
    let mut best = None;
    let mut best_area = 0u64;
    for track in tracks {
        if let Cr3TrackKind::Raw { width, height, .. } = track.kind {
            let area = u64::from(width) * u64::from(height);
            if area > best_area {
                best = Some(track);
                best_area = area;
            }
        }
    }
    best
}

fn cfa(pattern: u8) -> Option<Cfa> {
    let name = match pattern {
        0 => "RGGB",
        1 => "GRBG",
        2 => "GBRG",
        3 => "BGGR",
        _ => return None,
    };
    Some(Cfa::bayer_static(name))
}

/// Inclusive offsets, clipped to the sample dimensions (some Canon crop-mode files record right
/// and bottom borders a few pixels beyond the encoded image).
fn inclusive_rect(bounds: [u16; 4], width: usize, height: usize) -> Option<Rect> {
    let [left, top, right, bottom] = bounds.map(usize::from);
    if left >= width || top >= height || right < left || bottom < top {
        return None;
    }
    let end_x = right.checked_add(1)?.min(width);
    let end_y = bottom.checked_add(1)?.min(height);
    Some(Rect::new(left, top, end_x.checked_sub(left)?, end_y.checked_sub(top)?))
}

fn relative_crop(crop: Rect, active: Rect) -> Option<Rect> {
    let x = crop.x.checked_sub(active.x)?;
    let y = crop.y.checked_sub(active.y)?;
    if crop.width < 2 || crop.height < 2 || x.checked_add(crop.width)? > active.width || y.checked_add(crop.height)? > active.height {
        return None;
    }
    Some(Rect::new(x, y, crop.width, crop.height))
}

fn sensor_crop(maker: Option<&Ifd>, width: usize, height: usize) -> Option<Rect> {
    let data = maker?.u64s(SENSOR_INFO)?;
    let bounds = data.get(5..9)?;
    inclusive_rect(
        [
            u16::try_from(*bounds.first()?).ok()?,
            u16::try_from(*bounds.get(1)?).ok()?,
            u16::try_from(*bounds.get(2)?).ok()?,
            u16::try_from(*bounds.get(3)?).ok()?,
        ],
        width,
        height,
    )
}

fn geometry(area: Option<&Cr3ImageArea>, maker: Option<&Ifd>, width: usize, height: usize) -> (Rect, Rect) {
    let area = area.filter(|a| usize::from(a.width) == width && usize::from(a.height) == height);
    let sensor = sensor_crop(maker, width, height);
    let active = area.and_then(|a| a.active).and_then(|b| inclusive_rect(b, width, height)).or(sensor).unwrap_or(Rect::new(0, 0, width, height));
    let crop = area.and_then(|a| inclusive_rect(a.crop, width, height)).or(sensor).and_then(|c| relative_crop(c, active)).unwrap_or(Rect::new(
        0,
        0,
        active.width,
        active.height,
    ));
    (active, crop)
}

#[derive(Default)]
struct CameraLevels {
    wb: Option<[f32; 3]>,
    black: Option<[f32; 4]>,
    white: Option<f32>,
    color: ColorData,
}

/// Word offsets of Canon's versioned `ColorData` array (ExifTool's published `ColorData9`–
/// `ColorData12` tables; the R6 Mark III's version 66 follows the `ColorData12` layout, verified
/// by its sane plane, black and white words and by the white-balance round trip below).
#[derive(Clone, Copy)]
struct ColorLayout {
    /// Word index of the as-shot white-balance plane; each plane is four RGGB levels then a Kelvin temperature.
    planes_at: usize,
    /// White-balance planes stored before the `ColorCalib` block.
    planes: usize,
    black_at: usize,
    white_at: usize,
}

fn color_layout(version: Option<u64>) -> Option<ColorLayout> {
    Some(match version? {
        16..=19 => ColorLayout { planes_at: 71, planes: 39, black_at: 329, white_at: 797 },
        32 | 33 => ColorLayout { planes_at: 85, planes: 39, black_at: 343, white_at: 811 },
        34 | 48 => ColorLayout { planes_at: 105, planes: 39, black_at: 363, white_at: 641 },
        64..=66 => ColorLayout { planes_at: 105, planes: 43, black_at: 383, white_at: 661 },
        _ => return None,
    })
}

/// Canon's versioned word arrays. Offsets are word indices from the published tag tables, not
/// byte offsets. The four as-shot WB values are R, G, G, B, independent of the CFA layout.
fn camera_levels(maker: Option<&Ifd>, bits: u8) -> CameraLevels {
    let Some(values) = maker.and_then(|m| m.u64s(COLOR_DATA)) else { return CameraLevels::default() };
    let Some(layout) = color_layout(values.first().copied()) else { return CameraLevels::default() };
    let (wb_at, black_at, white_at) = (layout.planes_at, layout.black_at, layout.white_at);
    let full = ((1u32 << u32::from(bits.clamp(1, 16))) - 1) as f32;
    let quadruple = |at: usize| -> Option<[f32; 4]> {
        let v = values.get(at..at + 4)?;
        Some([*v.first()? as f32, *v.get(1)? as f32, *v.get(2)? as f32, *v.get(3)? as f32])
    };
    let wb = quadruple(wb_at).and_then(|[r, g1, g2, b]| {
        let green = (g1 + g2) * 0.5;
        if !(64.0..=32767.0).contains(&green) || (g1 - g2).abs() > green * 0.1 {
            return None;
        }
        let red = r / green;
        let blue = b / green;
        ((0.1..8.0).contains(&red) && (0.1..8.0).contains(&blue)).then_some([red, 1.0, blue])
    });
    let black = quadruple(black_at).filter(|v| v.iter().any(|&b| b > 0.0) && v.iter().all(|&b| b >= 0.0 && b < full * 0.5));
    let max_black = black.map(|v| v.into_iter().fold(0.0f32, f32::max)).unwrap_or(0.0);
    let white = values.get(white_at).map(|&v| v as f32).filter(|&v| v > max_black && v >= full * 0.5 && v <= full);
    let color = camera_color(&values, layout).unwrap_or_default();
    CameraLevels { wb, black, white, color }
}

/// One white-balance plane: the camera's own rendition of a neutral surface under a recorded
/// illuminant. The stored RGGB levels are the white-balance gains (×1024), so the camera neutral
/// — the illuminant's colour in camera RGB, what the colour matrix maps XYZ to — is their
/// reciprocal, green-normalised (`[G/R, 1, G/B]`).
struct WbPlane {
    neutral: [f64; 3],
    kelvin: f64,
}

/// The file's white-balance planes (Canon's named presets and measured shots, deduplicated).
fn wb_planes(values: &[u64], layout: ColorLayout) -> Vec<WbPlane> {
    let mut planes: Vec<WbPlane> = Vec::new();
    for i in 0..layout.planes {
        let at = layout.planes_at + i * 5;
        let Some(plane) = values.get(at..at + 5) else { break };
        let (r, g1, g2, b, kelvin) = (plane[0] as f64, plane[1] as f64, plane[2] as f64, plane[3] as f64, plane[4] as f64);
        let green = (g1 + g2) * 0.5;
        let (red, blue) = (r / green, b / green);
        // Placeholders, never real illuminants: 0x100-gain planes with impossible equal red and
        // blue gains, and identity-gain planes carrying no colour information. Real measured and
        // preset gains stay at or above 0x200 per channel and differ between the colour channels.
        let sane = (1500.0..=30000.0).contains(&kelvin)
            && (64.0..=32767.0).contains(&green)
            && (g1 - g2).abs() <= green * 0.1
            && r >= 512.0
            && b >= 512.0
            && r != b
            && !(r == green && b == green)
            && (0.1..7.0).contains(&red)
            && (0.1..7.0).contains(&blue);
        let neutral = [green / r, 1.0, green / b];
        if sane && !planes.iter().any(|p| p.kelvin == kelvin && p.neutral[0] == neutral[0] && p.neutral[2] == neutral[2]) {
            planes.push(WbPlane { neutral, kelvin });
        }
    }
    planes
}

/// Temperature (K) separating the two calibration-illuminant plane groups: tungsten-side planes
/// fit matrix 1, daylight-side planes matrix 2 (DNG's dual-illuminant model).
const WARM_SIDE: f64 = 4750.0;
/// Calibration illuminants of the derived matrices: standard light A and D65 (Exif codes 17, 21).
const ILLUMINANTS: [u16; 2] = [17, 21];

/// The white point used to normalise the fitted matrices (the green row maps it to 1).
fn reference_xy() -> Xy {
    cct::temp_tint_to_xy(6504.0, 0.0)
}

/// The chromaticity the file records for a plane's Kelvin temperature.
fn plane_xy(kelvin: f64) -> Xy {
    cct::temp_tint_to_xy(kelvin, 0.0)
}

/// Fit one XYZ → camera matrix to a plane group: alternating least squares over the neutral
/// ratios (each plane requires the red and blue rows to agree with its neutral relative to the
/// green row: `m_red·x − n_red·(m_green·x) = 0`, likewise blue). With the green row fixed each
/// row is an ordinary linear solve; the plane ratios only constrain the matrix on the Planckian
/// locus's narrow band, so each solve is ridged toward the generic camera (the pipeline's own
/// neutral fallback) on the locus's unconstrained directions. Grossly inconsistent planes
/// (Canon's fluorescent and placeholder planes) are dropped once and the fit repeated.
fn fit_plane_matrix(planes: &[(&WbPlane, Xy)]) -> Option<Mat3> {
    fit_plane_core(planes).ok()
}

/// Why a plane group produced no matrix, with the failed check's measurement.
type PlaneRejection = (&'static str, f64);

/// Ridge weight relative to a solve's data scale.
const RIDGE: f64 = 1e-3;
/// Alternating least squares passes (each is exact given the other rows; the iteration breaks on
/// its first degenerate pass and stops early once settled).
const ALS_PASSES: usize = 100;
/// Smallest plane group that still carries colour information of its own.
const MIN_GROUP: usize = 4;

fn fit_plane_core(planes: &[(&WbPlane, Xy)]) -> std::result::Result<Mat3, PlaneRejection> {
    let group: Vec<&(&WbPlane, Xy)> = planes.iter().collect();
    if group.len() < MIN_GROUP {
        return Err(("planes", group.len() as f64));
    }
    let mut matrix = als_fit(&group);
    if group.len() > MIN_GROUP + 2 {
        // One outlier pass: grossly inconsistent planes (Canon's fluorescent presets, placeholder
        // entries) would bias a least-squares fit; drop them and refit once, keeping the untrimmed
        // fit when the survivors are too few to determine a matrix.
        let mut errors: Vec<f64> = group.iter().filter_map(|(p, w)| direct_error(&matrix, p, *w)).collect();
        errors.sort_by(|a, b| a.total_cmp(b));
        let median = errors.get(errors.len() / 2).copied().unwrap_or(0.0);
        let cut = (median * 3.0).max(MAX_PLANE_ERROR);
        let survivors: Vec<&(&WbPlane, Xy)> = group.iter().copied().filter(|(p, w)| direct_error(&matrix, p, *w).is_some_and(|e| e <= cut)).collect();
        if survivors.len() < group.len() && survivors.len() > MIN_GROUP {
            matrix = als_fit(&survivors);
        }
    }
    let scale = matrix.0[1].iter().zip(reference_xy().to_xyz()).map(|(m, x)| m * x).sum::<f64>();
    if !scale.is_finite() || scale.abs() < 1e-9 {
        return Err(("scale", scale));
    }
    let normalized = Mat3(matrix.0.map(|r| r.map(|v| v / scale)));
    normalized.inverse().map(|_| normalized).ok_or(("final singular", scale))
}

/// The generic camera the fits fall back to on the locus's unconstrained directions: linear sRGB
/// as XYZ → camera, green row normalised at the reference white like a fitted matrix.
fn prior_matrix() -> Mat3 {
    let m = SRGB.from_xyz();
    let scale = m.0[1].iter().zip(reference_xy().to_xyz()).map(|(v, x)| v * x).sum::<f64>();
    if !scale.is_finite() || scale.abs() < 1e-9 {
        return m;
    }
    Mat3(m.0.map(|r| r.map(|v| v / scale)))
}

fn als_fit(group: &[&(&WbPlane, Xy)]) -> Mat3 {
    let prior = prior_matrix();
    let mut green = prior.0[1];
    let mut sane = prior;
    for _ in 0..ALS_PASSES {
        // A failed linear solve cannot update the fixed row, so the iteration stops; `sane`
        // keeps the last usable iterate (the prior until one succeeds) and the caller's plane
        // validation decides whether that tracks the file's planes well enough to use.
        let Some(red) = solve_row(group, &green, 0, &prior.0[0]) else { break };
        let Some(blue) = solve_row(group, &green, 2, &prior.0[2]) else { break };
        let Some(next) = solve_green(group, &red, &blue, &prior.0[1]) else { break };
        let matrix = Mat3([red, next, blue]);
        let settled = sane.0.iter().flatten().zip(matrix.0.iter().flatten()).all(|(a, b)| (a - b).abs() <= 1e-12 * a.abs().max(1.0));
        green = next;
        // Usable = finite, bounded, invertible, and its green row still maps the reference white
        // (the scale the caller normalises by): a narrow plane set can pass through degenerate,
        // row-dependent iterates on its way to the fixed point, so they are recorded but the
        // iteration continues from them.
        let scale = matrix.0[1].iter().zip(reference_xy().to_xyz()).map(|(m, x)| m * x).sum::<f64>();
        let bounded = matrix.0.iter().flatten().all(|v| v.is_finite() && v.abs() < 8.0);
        if bounded && scale.abs() > 1e-6 && matrix.inverse().is_some() && Mat3(matrix.0.map(|r| r.map(|v| v / scale))).inverse().is_some() {
            sane = matrix;
        }
        if settled {
            break;
        }
    }
    sane
}

/// `min Σ (m·x − n·(green·x))² + ridge·‖m − prior‖²` for one colour row.
fn solve_row(group: &[&(&WbPlane, Xy)], green: &[f64; 3], row: usize, prior: &[f64; 3]) -> Option<[f64; 3]> {
    let mut a = [[0.0f64; 3]; 3];
    let mut b = [0.0f64; 3];
    for (plane, white) in group {
        let x = white.to_xyz();
        let target = plane.neutral[row] * green.iter().zip(x).map(|(g, x)| g * x).sum::<f64>();
        for (i, &xi) in x.iter().enumerate() {
            b[i] += xi * target;
            for (j, &xj) in x.iter().enumerate() {
                a[i][j] += xi * xj;
            }
        }
    }
    solve_ridged(&a, &b, prior)
}

/// The green row from both other rows' ratio equations, likewise ridged.
fn solve_green(group: &[&(&WbPlane, Xy)], red: &[f64; 3], blue: &[f64; 3], prior: &[f64; 3]) -> Option<[f64; 3]> {
    let mut a = [[0.0f64; 3]; 3];
    let mut b = [0.0f64; 3];
    for (plane, white) in group {
        let x = white.to_xyz();
        for (row, ratio) in [(red, plane.neutral[0]), (blue, plane.neutral[2])] {
            let target = row.iter().zip(x).map(|(m, x)| m * x).sum::<f64>() / ratio;
            for (i, &xi) in x.iter().enumerate() {
                b[i] += xi * target;
                for (j, &xj) in x.iter().enumerate() {
                    a[i][j] += xi * xj;
                }
            }
        }
    }
    solve_ridged(&a, &b, prior)
}

/// `(A + ridge·tr(A)/3·I)·m = b + ridge·tr(A)/3·prior` (data-scale-relative Tikhonov).
fn solve_ridged(a: &[[f64; 3]; 3], b: &[f64; 3], prior: &[f64; 3]) -> Option<[f64; 3]> {
    let trace: f64 = a[0][0] + a[1][1] + a[2][2];
    let ridge = trace * RIDGE / 3.0;
    let mut ridged = *a;
    for i in 0..3 {
        ridged[i][i] += ridge;
    }
    let inverse = Mat3(ridged).inverse()?;
    let rhs = [b[0] + ridge * prior[0], b[1] + ridge * prior[1], b[2] + ridge * prior[2]];
    let m = inverse.apply(rhs);
    m.iter().all(|v| v.is_finite() && v.abs() < 8.0).then_some(m)
}

/// Relative neutral error of `matrix` at one plane (its own group's matrix, no interpolation).
fn direct_error(matrix: &Mat3, plane: &WbPlane, white: Xy) -> Option<f64> {
    let predicted = matrix.apply(white.to_xyz());
    let pg = predicted[1];
    if pg <= 0.0 {
        return None;
    }
    let error = [0, 2].into_iter().map(|c| (predicted[c] * plane.neutral[1] / (pg * plane.neutral[c]) - 1.0).abs()).fold(0.0f64, f64::max);
    error.is_finite().then_some(error)
}

/// Relative per-plane error of the derived colour model: how far the matrix's white balance for
/// the plane's recorded illuminant is from the plane's own multipliers (max of red and blue).
fn plane_error(color: &ColorData, plane: &WbPlane) -> Option<f64> {
    let predicted = crate::color::camera_neutral(color, plane_xy(plane.kelvin));
    let (g, pg) = (plane.neutral[1], predicted[1]);
    if g <= 0.0 || pg <= 0.0 {
        return None;
    }
    let error = [0, 2].into_iter().map(|c| (predicted[c] * g / (pg * plane.neutral[c]) - 1.0).abs());
    let max = error.fold(0.0f64, f64::max);
    max.is_finite().then_some(max)
}

/// Mean and max per-plane error of `color` over all planes.
fn plane_errors(color: &ColorData, planes: &[WbPlane]) -> (f64, f64) {
    let mut count = 0usize;
    let mut sum = 0.0;
    let mut max = 0.0f64;
    for plane in planes {
        let Some(error) = plane_error(color, plane) else { continue };
        count += 1;
        sum += error;
        max = max.max(error);
    }
    if count == 0 { (f64::INFINITY, f64::INFINITY) } else { (sum / count as f64, max) }
}

/// Planes must fit this well (relative white-balance error) for the matrices to be used. The
/// generic camera fallback these replace is typically several times worse, and a wrong layout
/// fails by an order of magnitude, so the bar is set by "clearly beats the fallback".
const MAX_MEAN_PLANE_ERROR: f64 = 0.08;
const MAX_PLANE_ERROR: f64 = 0.25;
/// The file's own as-shot plane (the white balance this photo ships with) must reproduce better.
const MAX_AS_SHOT_PLANE_ERROR: f64 = 0.08;

/// The file's colour model: XYZ → camera matrices fitted to its own white-balance planes,
/// validated by reproducing them (and the as-shot plane) within tolerance. `None` keeps the
/// embedded-JPEG colour fallback (unknown layout, too few planes, or a failed validation).
fn camera_color(values: &[u64], layout: ColorLayout) -> Option<ColorData> {
    camera_color_core(values, layout).ok()
}

fn camera_color_core(values: &[u64], layout: ColorLayout) -> std::result::Result<ColorData, PlaneRejection> {
    let planes = wb_planes(values, layout);
    let xy_of = |p: &WbPlane| plane_xy(p.kelvin);
    let warm: Vec<_> = planes.iter().filter(|p| p.kelvin < WARM_SIDE).map(|p| (p, xy_of(p))).collect();
    let cool: Vec<_> = planes.iter().filter(|p| p.kelvin >= WARM_SIDE).map(|p| (p, xy_of(p))).collect();
    // Both calibration matrices from their temperature sides; a side with too few planes falls
    // back to the global matrix over all planes (never to a far-extrapolating other side).
    let global = fit_plane_matrix(&planes.iter().map(|p| (p, xy_of(p))).collect::<Vec<_>>());
    let m1 = fit_plane_matrix(&warm).or(global).ok_or(("fit", 0.0))?;
    let m2 = fit_plane_matrix(&cool).or(global).ok_or(("fit", 0.0))?;
    let color = ColorData {
        illuminant: ILLUMINANTS,
        color_matrix: [Some(m1), Some(m2)],
        forward_matrix: [None, None],
        camera_calibration: [None, None],
        analog_balance: None,
        as_shot_neutral: None,
        as_shot_white_xy: None,
        baseline_exposure: 0.0,
        profile: Default::default(),
    };
    let (mean, max) = plane_errors(&color, &planes);
    if !mean.is_finite() {
        return Err(("planes", mean));
    }
    if mean > MAX_MEAN_PLANE_ERROR {
        return Err(("mean", mean));
    }
    if max > MAX_PLANE_ERROR {
        return Err(("max", max));
    }
    // The as-shot plane is the photo's own white balance: reproduce it, not just the family.
    let shot = plane_error(&color, planes.first().ok_or(("planes", 0.0))?).ok_or(("as shot", f64::NAN))?;
    if shot > MAX_AS_SHOT_PLANE_ERROR {
        return Err(("as shot", shot));
    }
    Ok(color)
}

fn black_at_active(levels: [f32; 4], pattern: u8, active: Rect) -> BlackLevel {
    // Convert Canon's channel order to full-sensor site order, then shift to the active origin.
    let sites = match pattern {
        1 => [1, 0, 3, 2],
        2 => [2, 3, 0, 1],
        3 => [3, 2, 1, 0],
        _ => [0, 1, 2, 3],
    };
    let values = (0..4)
        .map(|i| {
            let x = (active.x + i % 2) % 2;
            let y = (active.y + i / 2) % 2;
            levels.get(*sites.get(y * 2 + x).unwrap_or(&0)).copied().unwrap_or(0.0)
        })
        .collect();
    BlackLevel { repeat_rows: 2, repeat_cols: 2, values, delta_h: vec![], delta_v: vec![] }
}

pub(crate) fn decode(bytes: &[u8], mode: Mode) -> Result<RawImage> {
    let container = parse_cr3(bytes).ok_or(RawError::NotRaw)?;
    let track = main_track(&container.tracks).ok_or_else(|| RawError::Unsupported("CR3 without a Bayer raw track".into()))?;
    let coding = track.compression(bytes).ok_or_else(|| RawError::Corrupt("CR3 raw track has an invalid CMP1 descriptor".into()))?;
    if coding.planes != 4 || coding.encoding != 0 {
        return Err(RawError::Unsupported(format!(
            "Canon CR3 encoding {} with {} planes; note: sRAW/mRAW are CR2-era YCbCr codings and do not occur in CR3",
            coding.encoding, coding.planes
        )));
    }
    let (width, height) = (coding.width as usize, coding.height as usize);
    let samples = width.checked_mul(height).filter(|&n| n > 0 && n <= MAX_SAMPLES).ok_or(RawError::Limit("CR3 sensor dimensions"))?;
    let cfa = cfa(coding.cfa_pattern).ok_or_else(|| RawError::Unsupported(format!("Canon CR3 CFA {}", coding.cfa_pattern)))?;
    let (offset, len) = track.data.ok_or_else(|| RawError::Corrupt("CR3 raw sample outside file".into()))?;
    let sample =
        offset.checked_add(len).and_then(|end| bytes.get(offset..end)).ok_or_else(|| RawError::Corrupt("CR3 raw sample outside file".into()))?;
    if coding.header_size as usize > sample.len() {
        return Err(RawError::Corrupt("CR3 coding header outside sample".into()));
    }
    crx::validate(sample, &coding)?;
    let data = match mode {
        Mode::Full => {
            let data = crx::decode(sample, &coding)?;
            if data.len() != samples {
                return Err(RawError::Corrupt("CR3 decoded sample count does not match CMP1".into()));
            }
            data
        }
        Mode::Header => vec![],
    };
    let static_maker = container.cmt.get(2).and_then(|b| *b).and_then(|b| Tiff::parse(b).ok());
    let maker = static_maker.as_ref().and_then(|m| m.ifds.first());
    // Retain only the first useful timed maker note: repeated CTMD records must not accumulate
    // independent copies of every bounded TIFF value in memory.
    let timed_maker = container
        .timed_exif(bytes)
        .into_iter()
        .filter(|b| b.tag == 0x927c)
        .filter_map(|b| Tiff::parse(b.data).ok())
        .find(|t| t.ifds.first().is_some_and(|m| m.contains(COLOR_DATA)));
    let color_maker = timed_maker.as_ref().and_then(|t| t.ifds.first()).or(maker);
    let area = track.image_area(bytes);
    let (active, crop) = geometry(area.as_ref(), maker, width, height);
    let levels = camera_levels(color_maker, coding.bit_depth);
    let black = match levels.black {
        Some(levels) => black_at_active(levels, coding.cfa_pattern, active),
        None if active.x >= 8 => black_from_columns(&data, width, 2..active.x - 2, active.y..active.y + active.height, active),
        None => BlackLevel::uniform(0.0),
    };
    let white = levels.white.unwrap_or_else(|| white_from_data(&data, u32::from(coding.bit_depth)));
    let mut metadata = lightcraft_meta::extract(bytes);
    if metadata.lens_model.is_none() {
        metadata.lens_model = maker.and_then(|m| m.string(LENS_MODEL)).map(|s| s.trim().to_owned()).filter(|s| !s.is_empty());
    }
    metadata.width = Some(crop.width as u32);
    metadata.height = Some(crop.height as u32);
    let image = RawImage {
        format: RawFormat::Cr3,
        width,
        height,
        cpp: 1,
        data: RawData::U16(data),
        cfa: Some(cfa),
        bits: u32::from(coding.bit_depth),
        black,
        white: vec![white],
        active_area: active,
        crop,
        orientation: metadata.orientation.unwrap_or_default(),
        color: levels.color,
        wb_multipliers: levels.wb,
        linearized: false,
        opcodes: OpcodeLists::default(),
        metadata,
    };
    image.validate_for(mode)?;
    Ok(image)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lightcraft_tiff::{IfdBuilder, Value};

    fn maker(version: u16, wb_at: usize, black_at: usize, white_at: usize) -> Ifd {
        let mut words = vec![0u16; white_at + 1];
        words[0] = version;
        words[wb_at..wb_at + 4].copy_from_slice(&[2048, 1024, 1024, 1536]);
        words[black_at..black_at + 4].copy_from_slice(&[100, 101, 102, 103]);
        words[white_at] = 15000;
        let b = IfdBuilder::new().with(COLOR_DATA, Value::Short(words));
        let bytes = lightcraft_tiff::TiffWriter::new(lightcraft_tiff::ByteOrder::Little, false).write(&[b]).unwrap();
        Tiff::parse(&bytes).unwrap().ifds.remove(0)
    }

    fn box_bytes(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut b = ((body.len() + 8) as u32).to_be_bytes().to_vec();
        b.extend_from_slice(kind);
        b.extend_from_slice(body);
        b
    }

    fn track(entry: &[u8], offset: u64, size: u32) -> Vec<u8> {
        let mut stsd = vec![0u8; 4];
        stsd.extend(1u32.to_be_bytes());
        stsd.extend(entry);
        let mut stsz = vec![0u8; 4];
        stsz.extend(size.to_be_bytes());
        stsz.extend(1u32.to_be_bytes());
        let mut co64 = vec![0u8; 4];
        co64.extend(1u32.to_be_bytes());
        co64.extend(offset.to_be_bytes());
        let tables = [box_bytes(b"stsd", &stsd), box_bytes(b"stsz", &stsz), box_bytes(b"co64", &co64)].concat();
        box_bytes(b"trak", &box_bytes(b"mdia", &box_bytes(b"minf", &box_bytes(b"stbl", &tables))))
    }

    fn synthetic_header() -> Vec<u8> {
        use lightcraft_tiff::{ByteOrder, TiffWriter, tags as t};
        let tiff = |b: IfdBuilder| TiffWriter::new(ByteOrder::Little, false).write(&[b]).unwrap();
        let cmt1 = tiff(
            IfdBuilder::new()
                .with(t::MAKE, Value::Ascii("Canon".into()))
                .with(t::MODEL, Value::Ascii("EOS Test".into()))
                .with(t::ORIENTATION, Value::Short(vec![6])),
        );
        let cmt2 = tiff(IfdBuilder::new().with(t::ISO_SPEED, Value::Short(vec![800])));
        let cmt3 = tiff(
            IfdBuilder::new()
                .with(SENSOR_INFO, Value::Short(vec![34, 64, 48, 1, 1, 6, 4, 61, 45]))
                .with(LENS_MODEL, Value::Ascii("RF-S18-45mm F4.5-6.3 IS STM".into())),
        );
        let mut canon = vec![0x85, 0xc0, 0xb6, 0x87, 0x82, 0x0f, 0x11, 0xe0, 0x81, 0x11, 0xf4, 0xce, 0x46, 0x2b, 0x6a, 0x48];
        canon.extend([box_bytes(b"CMT1", &cmt1), box_bytes(b"CMT2", &cmt2), box_bytes(b"CMT3", &cmt3)].concat());
        let mut coding = vec![0u8; 52];
        coding[0..2].copy_from_slice(&0xff00u16.to_be_bytes());
        coding[2..4].copy_from_slice(&48u16.to_be_bytes());
        coding[4..6].copy_from_slice(&0x100u16.to_be_bytes());
        for (at, value) in [(8, 64u32), (12, 48), (16, 32), (20, 48), (28, 216)] {
            coding[at..at + 4].copy_from_slice(&value.to_be_bytes());
        }
        coding[24] = 14;
        coding[25] = 0x40;
        let mut area = vec![0u8; 48];
        for (at, value) in [
            (4, 64u16),
            (6, 48),
            (10, 2),
            (16, 6),
            (18, 4),
            (20, 61),
            (22, 45),
            (28, 3),
            (30, 47),
            (32, 4),
            (36, 63),
            (38, 1),
            (40, 4),
            (42, 2),
            (44, 63),
            (46, 47),
        ] {
            area[at..at + 2].copy_from_slice(&value.to_be_bytes());
        }
        let mut craw = vec![0u8; 82];
        craw[24..26].copy_from_slice(&64u16.to_be_bytes());
        craw[26..28].copy_from_slice(&48u16.to_be_bytes());
        craw.extend(box_bytes(b"CMP1", &coding));
        craw.extend(box_bytes(b"CDI1", &[vec![0u8; 4], box_bytes(b"IAD1", &area)].concat()));
        let levels = maker(48, 105, 363, 641).value(COLOR_DATA).unwrap().clone();
        let dynamic_maker = tiff(IfdBuilder::new().with(COLOR_DATA, levels));
        let mut item = ((dynamic_maker.len() + 8) as u32).to_le_bytes().to_vec();
        item.extend(0x927cu32.to_le_bytes());
        item.extend(dynamic_maker);
        let mut timed = vec![0u8; 12];
        timed[0..4].copy_from_slice(&((12 + item.len()) as u32).to_le_bytes());
        timed[4..6].copy_from_slice(&8u16.to_le_bytes());
        timed.extend(item);
        // Two tiles, each with four one-byte lossless plane streams. Header
        // probing checks the complete marker layout without decoding entropy.
        let mut sample = Vec::new();
        let marker = |out: &mut Vec<u8>, code: u16, size: u32, flags: u32| {
            out.extend(code.to_be_bytes());
            out.extend(8u16.to_be_bytes());
            out.extend(size.to_be_bytes());
            out.extend(flags.to_be_bytes());
        };
        for tile in 0..2 {
            marker(&mut sample, 0xff01, 4, tile << 16);
            for plane in 0..4 {
                marker(&mut sample, 0xff02, 1, (plane << 28) | 0x0800_0000);
                marker(&mut sample, 0xff03, 1, 0x0020_0000);
            }
        }
        assert_eq!(sample.len(), 216);
        sample.extend([0x1e; 8]);
        let moov =
            [box_bytes(b"uuid", &canon), track(&box_bytes(b"CRAW", &craw), 4096, 224), track(&box_bytes(b"CTMD", &[]), 4320, timed.len() as u32)]
                .concat();
        let mut file = box_bytes(b"ftyp", b"crx \0\0\0\x01crx isom");
        file.extend(box_bytes(b"moov", &moov));
        assert!(file.len() < 4096);
        file.resize(4096, 0);
        file.extend(sample);
        file.extend(timed);
        file
    }

    #[test]
    fn header_reads_timed_white_balance_and_distinct_geometry() {
        let bytes = synthetic_header();
        let image = decode(&bytes, Mode::Header).unwrap();
        assert_eq!((image.width, image.height, image.bits), (64, 48, 14));
        assert_eq!(image.active_area, Rect::new(4, 2, 60, 46));
        assert_eq!(image.crop, Rect::new(2, 2, 56, 42));
        assert_eq!(image.wb_multipliers, Some([2.0, 1.0, 1.5]));
        assert_eq!(image.black.values, vec![100.0, 101.0, 102.0, 103.0]);
        assert_eq!(image.white, vec![15000.0]);
        assert_eq!(image.metadata.iso, Some(800));
        assert_eq!(image.metadata.lens_model.as_deref(), Some("RF-S18-45mm F4.5-6.3 IS STM"));
        assert_eq!((image.metadata.width, image.metadata.height), (Some(56), Some(42)));
        assert_eq!(image.orientation, crate::Orientation::Rotate90);
        assert!(image.data.is_empty());
        assert!(decode(&bytes[..4200], Mode::Header).is_err());
    }

    fn prepend_tracks(bytes: &[u8], tracks: &[u8]) -> Vec<u8> {
        let ftyp_end = u32::from_be_bytes(bytes[..4].try_into().unwrap()) as usize;
        let moov_len = u32::from_be_bytes(bytes[ftyp_end..ftyp_end + 4].try_into().unwrap()) as usize;
        assert_eq!(&bytes[ftyp_end + 4..ftyp_end + 8], b"moov");
        let moov = [tracks, &bytes[ftyp_end + 8..ftyp_end + moov_len]].concat();
        let mut out = [bytes[..ftyp_end].to_vec(), box_bytes(b"moov", &moov)].concat();
        assert!(out.len() < 4096);
        out.resize(4096, 0);
        out.extend_from_slice(&bytes[4096..]);
        out
    }

    #[test]
    fn hevc_preview_with_equal_dimensions_does_not_replace_the_sensor_track() {
        let bytes = synthetic_header();
        let mut preview = vec![0u8; 82];
        preview[24..26].copy_from_slice(&64u16.to_be_bytes());
        preview[26..28].copy_from_slice(&48u16.to_be_bytes());
        preview.extend(box_bytes(b"HEVC", &[0; 4]));
        let bytes = prepend_tracks(&bytes, &track(&box_bytes(b"CRAW", &preview), 4096, 224));
        let container = parse_cr3(&bytes).unwrap();
        assert_eq!(container.tracks[0].kind, Cr3TrackKind::Other(*b"HEVC"));
        let image = decode(&bytes, Mode::Header).unwrap();
        assert_eq!((image.width, image.height, image.bits), (64, 48, 14));
        assert_eq!(image.active_area, Rect::new(4, 2, 60, 46));
    }

    #[test]
    fn corrupt_main_descriptor_is_not_replaced_by_the_reduced_raw() {
        let bytes = synthetic_header();
        let container = parse_cr3(&bytes).unwrap();
        let Cr3TrackKind::Raw { cmp1: Some((at, len)), .. } = main_track(&container.tracks).unwrap().kind else { panic!("raw fixture") };
        let mut reduced = vec![0u8; 82];
        reduced[24..26].copy_from_slice(&32u16.to_be_bytes());
        reduced[26..28].copy_from_slice(&24u16.to_be_bytes());
        reduced.extend(box_bytes(b"CMP1", &bytes[at..at + len]));
        let bytes = prepend_tracks(&bytes, &track(&box_bytes(b"CRAW", &reduced), 4096, 224));
        let container = parse_cr3(&bytes).unwrap();
        let Cr3TrackKind::Raw { cmp1: Some((at, len)), .. } = main_track(&container.tracks).unwrap().kind else { panic!("main raw fixture") };
        let mut malformed = bytes.clone();
        malformed[at..at + len].fill(0);
        assert!(matches!(decode(&malformed, Mode::Header), Err(RawError::Corrupt(_))));
        let mut missing = bytes;
        missing[at - 4..at].copy_from_slice(b"free");
        assert!(matches!(decode(&missing, Mode::Header), Err(RawError::Corrupt(_))));
    }

    #[test]
    fn color_data_uses_versioned_as_shot_offsets() {
        for (version, wb_at, black_at, white_at) in
            [(16, 71, 329, 797), (19, 71, 329, 797), (33, 85, 343, 811), (48, 105, 363, 641), (64, 105, 383, 661)]
        {
            let m = maker(version, wb_at, black_at, white_at);
            let levels = camera_levels(Some(&m), 14);
            assert_eq!(levels.wb, Some([2.0, 1.0, 1.5]));
            assert_eq!(levels.black, Some([100.0, 101.0, 102.0, 103.0]));
            assert_eq!(levels.white, Some(15000.0));
        }
        assert!(camera_levels(Some(&maker(255, 71, 329, 797)), 14).wb.is_none());
        assert!(camera_levels(None, 14).black.is_none());
    }

    #[test]
    fn crop_is_relative_to_the_valid_sensor_area() {
        let area = Cr3ImageArea {
            width: 6888,
            height: 4546,
            crop: [156, 158, 6875, 4537],
            active: Some([144, 46, 6887, 4545]),
            masked_left: [0, 0, 143, 4545],
            masked_top: None,
        };
        assert_eq!(geometry(Some(&area), None, 6888, 4546), (Rect::new(144, 46, 6744, 4500), Rect::new(12, 112, 6720, 4380)));
        assert_eq!(inclusive_rect([152, 46, 4353, 2851], 4352, 2850), Some(Rect::new(152, 46, 4200, 2804)));
        assert_eq!(inclusive_rect([100, 100, 50, 50], 200, 200), None);
        assert_eq!(relative_crop(Rect::new(0, 0, 100, 100), Rect::new(4, 4, 92, 92)), None);
    }

    #[test]
    fn first_full_size_track_wins_over_dual_pixel_delta() {
        let raw = |w, h, offset| Cr3Track { kind: Cr3TrackKind::Raw { width: w, height: h, cmp1: None, iad1: None }, data: Some((offset, 100)) };
        let tracks = [raw(1624, 1080, 1), raw(6888, 4546, 2), raw(6888, 4546, 3)];
        assert_eq!(main_track(&tracks).unwrap().data, Some((2, 100)));
    }

    #[test]
    fn cfa_and_black_follow_the_full_sensor_origin() {
        for (code, name) in [(0, "RGGB"), (1, "GRBG"), (2, "GBRG"), (3, "BGGR")] {
            assert_eq!(cfa(code).unwrap().name(), name);
        }
        let b = black_at_active([100.0, 101.0, 102.0, 103.0], 0, Rect::new(3, 1, 32, 20));
        assert_eq!(b.values, vec![103.0, 102.0, 101.0, 100.0]);
    }

    /// The fit must recover a synthetic camera matrix from Planckian planes. The ridge toward the
    /// generic camera leaves a small equilibrium bias for a matrix this far from the prior (the
    /// planes only constrain the matrix on the locus's narrow band), so the assertion is the
    /// production bar: locus predictions within a few percent, the same relative-error shape the
    /// plane validation gates on.
    #[test]
    fn synthetic_fit_recovers_matrix() {
        let m = Mat3([[0.9, 0.2, -0.15], [-0.3, 1.25, 0.08], [0.02, -0.12, 0.85]]);
        let mk = |ts: &[f64]| -> Vec<WbPlane> {
            ts.iter()
                .map(|&t| {
                    let n = m.apply(plane_xy(t).to_xyz());
                    let g = n[1];
                    WbPlane { neutral: [n[0] / g, 1.0, n[2] / g], kelvin: t }
                })
                .collect()
        };
        let warm_planes = mk(&[2600.0, 3200.0, 3700.0, 4200.0]);
        let cool_planes = mk(&[5200.0, 6000.0, 7000.0, 8000.0]);
        let warm: Vec<(&WbPlane, Xy)> = warm_planes.iter().map(|p| (p, plane_xy(p.kelvin))).collect();
        let cool: Vec<(&WbPlane, Xy)> = cool_planes.iter().map(|p| (p, plane_xy(p.kelvin))).collect();
        for (name, group) in [("warm", &warm), ("cool", &cool)] {
            let fitted = fit_plane_core(group).unwrap_or_else(|reason| panic!("{name} fit failed: {reason:?}"));
            for (plane, white) in group {
                let error = direct_error(&fitted, plane, *white).expect("plane error finite");
                assert!(error < 0.06, "{name} @{}: plane error {error}", plane.kelvin);
            }
            // Predictions inside the group's temperature span are what rendering usually sees
            // (white points and the illuminant interpolation between the two calibrants). Past
            // the end planes the prior takes over and the error grows — asserted only to stay
            // bounded (the production max-plane bar), never exploding into a wrong matrix.
            let (inside, outside) = if name == "warm" { ([3000.0, 4000.0], [2300.0, 4500.0]) } else { ([5500.0, 7500.0], [5000.0, 9000.0]) };
            for (t, limit) in inside.iter().map(|&t| (t, 0.05)).chain(outside.iter().map(|&t| (t, MAX_PLANE_ERROR))) {
                let a = fitted.apply(plane_xy(t).to_xyz());
                let b = m.apply(plane_xy(t).to_xyz());
                let k = a[1] / b[1];
                for c in [0, 2] {
                    let relative = (a[c] / k - b[c]).abs() / b[c].abs();
                    assert!(relative < limit, "{name} @{t}: channel {c} off by {relative:.4} (limit {limit})");
                }
            }
        }
    }
}
