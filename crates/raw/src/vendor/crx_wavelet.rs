//! Reversible integer Le Gall 5/3 synthesis for Canon CRX subbands.
//!
//! Independent implementation of the lifting equations in ITU-T T.800, Annex F;
//! the equations are also printed in US20030031370A1, paragraph 0047.
//! Each tile level has even local origin. Internal boundaries retain the
//! neighbouring low/high coefficients; only outer image edges are symmetric.

use crate::{RawError, Result};

#[derive(Clone, Debug)]
pub(crate) struct Band {
    pub width: usize,
    pub height: usize,
    pub data: Vec<i32>,
}

fn corrupt() -> RawError {
    RawError::Corrupt("invalid CRX wavelet subband geometry".into())
}

fn checked_sample(v: i64) -> Result<i32> {
    i32::try_from(v).map_err(|_| RawError::Corrupt("CRX wavelet coefficient overflow".into()))
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct TileEdges {
    pub left: bool,
    pub right: bool,
    pub top: bool,
    pub bottom: bool,
}

fn sizes(length: usize, before: bool, after: bool) -> (usize, usize) {
    (length.div_ceil(2) + usize::from(after && length.is_multiple_of(2)), length / 2 + usize::from(before) + usize::from(after))
}

fn inverse_line(low: &[i32], high: &[i32], out: &mut [i32], before: bool, after: bool) -> Result<()> {
    let (low_count, high_count) = sizes(out.len(), before, after);
    if out.is_empty() || low.len() != low_count || high.len() != high_count {
        return Err(corrupt());
    }
    if high.is_empty() {
        let value = low.first().copied().ok_or_else(corrupt)?;
        *out.first_mut().ok_or_else(corrupt)? = value;
        return Ok(());
    }
    let n = out.len();
    let (evens, odds) = (n.div_ceil(2), n / 2);
    let neighbour = usize::from(before);
    // The high-pass storage starts at -1 when there is a preceding tile.
    // Only absent external neighbours are reflected at the image boundary.
    let high_at = |index: usize| -> Result<i32> { high.get(index.min(high.len().saturating_sub(1))).copied().ok_or_else(corrupt) };
    let even = |i: usize| -> Result<i32> {
        let left = high_at((i + neighbour).saturating_sub(1))?;
        let right = high_at(i + neighbour)?;
        let update = (i64::from(left) + i64::from(right) + 2).div_euclid(4);
        checked_sample(i64::from(*low.get(i).ok_or_else(corrupt)?) - update)
    };
    // Even synthesis first, into a scratch of its own: the odd pass then reads
    // neighbouring evens as two sequential streams while interleaving both
    // parities back into `out`. The clamped-neighbourhood helper above is used
    // only where a neighbour can actually reflect or clamp (even index 0 and
    // the last even); for every interior index `i` the neighbours
    // `i + neighbour - 1` and `i + neighbour` provably lie inside `high`, so
    // the interior runs as plain iterator windows. All arithmetic is the
    // lifting equations verbatim, evaluated in ascending index order as before.
    let mut synthesis = zeroed(evens)?;
    *synthesis.first_mut().ok_or_else(corrupt)? = even(0)?;
    let interior = evens.saturating_sub(2);
    let values = low.get(1..1 + interior).ok_or_else(corrupt)?;
    let lefts = high.get(neighbour..neighbour + interior).ok_or_else(corrupt)?;
    let rights = high.get(neighbour + 1..neighbour + 1 + interior).ok_or_else(corrupt)?;
    for (((slot, &value), &left), &right) in synthesis.iter_mut().skip(1).zip(values).zip(lefts).zip(rights) {
        let update = (i64::from(left) + i64::from(right) + 2).div_euclid(4);
        *slot = checked_sample(i64::from(value) - update)?;
    }
    if evens > 1 {
        let slot = synthesis.get_mut(evens - 1).ok_or_else(corrupt)?;
        *slot = even(evens - 1)?;
    }
    // Odd synthesis. Interior odd indices read `high` directly (`i + neighbour`
    // is provably in range) and consecutive evens from the scratch; an even
    // output length leaves the last odd with a reflected/absent right
    // neighbour, handled through the same helper expressions as before.
    let odds_interior = odds - usize::from(n.is_multiple_of(2));
    let highs = high.get(neighbour..neighbour + odds_interior).ok_or_else(corrupt)?;
    let followers = synthesis.get(1..).ok_or_else(corrupt)?;
    let (pairs, _) = out.as_chunks_mut::<2>();
    for (((pair, &even_value), &next_even), &high_value) in
        pairs.iter_mut().zip(synthesis.iter()).zip(followers.iter()).zip(highs)
    {
        let prediction = (i64::from(even_value) + i64::from(next_even)).div_euclid(2);
        pair[0] = even_value;
        pair[1] = checked_sample(i64::from(high_value) + prediction)?;
    }
    if n.is_multiple_of(2) {
        let pair = pairs.last_mut().ok_or_else(corrupt)?;
        let left = *synthesis.get(odds - 1).ok_or_else(corrupt)?;
        *pair.first_mut().ok_or_else(corrupt)? = left;
        let right = if after { even(odds)? } else { left };
        let prediction = (i64::from(left) + i64::from(right)).div_euclid(2);
        let value = high_at(odds - 1 + neighbour)?;
        *pair.get_mut(1).ok_or_else(corrupt)? = checked_sample(i64::from(value) + prediction)?;
    } else {
        *out.last_mut().ok_or_else(corrupt)? = *synthesis.get(odds).ok_or_else(corrupt)?;
    }
    Ok(())
}

fn valid_band(band: &Band, width: usize, height: usize) -> bool {
    band.width == width && band.height == height && width.checked_mul(height) == Some(band.data.len())
}

/// A zeroed buffer, or a limit error instead of an abort when the allocation fails.
fn zeroed(count: usize) -> Result<Vec<i32>> {
    let mut v = Vec::new();
    v.try_reserve_exact(count).map_err(|_| RawError::Limit("CRX wavelet allocation"))?;
    v.resize(count, 0);
    Ok(v)
}

/// One inverse level. `HL` is horizontally high-pass, `LH` vertically high-pass.
pub(crate) fn inverse_level(ll: &Band, hl: &Band, lh: &Band, hh: &Band, width: usize, height: usize, edges: TileEdges) -> Result<Band> {
    let count = width.checked_mul(height).filter(|n| *n > 0).ok_or_else(corrupt)?;
    if count > crate::MAX_SAMPLES {
        return Err(RawError::Limit("CRX wavelet image size"));
    }
    let (lw, hw) = sizes(width, edges.left, edges.right);
    let (lhgt, hhgt) = sizes(height, edges.top, edges.bottom);
    if !valid_band(ll, lw, lhgt) || !valid_band(hl, hw, lhgt) || !valid_band(lh, lw, hhgt) || !valid_band(hh, hw, hhgt) {
        return Err(corrupt());
    }
    let intermediate_count = width.checked_mul(lhgt + hhgt).filter(|n| *n <= crate::MAX_SAMPLES).ok_or(RawError::Limit("CRX wavelet image size"))?;
    let mut temporary = zeroed(intermediate_count)?;
    // CRX analysis is vertical then horizontal. Integer rounding means that
    // synthesis must reverse that order, including for quantized subbands.
    for y in 0..lhgt + hhgt {
        let (lo, hi, by) = if y < lhgt { (ll, hl, y) } else { (lh, hh, y - lhgt) };
        inverse_line(
            lo.data.get(by * lw..(by + 1) * lw).ok_or_else(corrupt)?,
            hi.data.get(by * hw..(by + 1) * hw).ok_or_else(corrupt)?,
            temporary.get_mut(y * width..(y + 1) * width).ok_or_else(corrupt)?,
            edges.left,
            edges.right,
        )?;
    }
    let mut data = zeroed(count)?;
    // The vertical lifting walks columns of the row-major intermediate. One
    // column at a time strides `width` elements between rows, using one cache
    // line (and page) per row for a single element. Instead, lift
    // COLUMN_GROUP neighbouring columns together: every gather and scatter
    // moves one contiguous run per row, so line (and page) touches shrink by
    // the group width. Groups are gathered, lifted and scattered one at a
    // time, so the scratch always holds exactly one group. The arithmetic per
    // column, and the column order, are unchanged.
    const COLUMN_GROUP: usize = 32;
    // Narrow planes cap the group, so the scratch never exceeds a small
    // multiple of the old per-column buffers regardless of the plane shape.
    let group = COLUMN_GROUP.min(width);
    let mut low = zeroed(group * lhgt)?;
    let mut high = zeroed(group * hhgt)?;
    let mut column = zeroed(group * height)?;
    for left in (0..width).step_by(COLUMN_GROUP) {
        let span = group.min(width - left);
        for (y, row) in temporary.chunks_exact(width).enumerate() {
            let values = row.get(left..left + span).ok_or_else(corrupt)?;
            if y < lhgt {
                for (c, &value) in values.iter().enumerate() {
                    *low.get_mut(c * lhgt + y).ok_or_else(corrupt)? = value;
                }
            } else {
                for (c, &value) in values.iter().enumerate() {
                    *high.get_mut(c * hhgt + y - lhgt).ok_or_else(corrupt)? = value;
                }
            }
        }
        for c in 0..span {
            inverse_line(
                low.get(c * lhgt..(c + 1) * lhgt).ok_or_else(corrupt)?,
                high.get(c * hhgt..(c + 1) * hhgt).ok_or_else(corrupt)?,
                column.get_mut(c * height..(c + 1) * height).ok_or_else(corrupt)?,
                edges.top,
                edges.bottom,
            )?;
        }
        for (y, row) in data.chunks_exact_mut(width).enumerate() {
            let values = row.get_mut(left..left + span).ok_or_else(corrupt)?;
            for (c, slot) in values.iter_mut().enumerate() {
                *slot = *column.get(c * height + y).ok_or_else(corrupt)?;
            }
        }
    }
    Ok(Band { width, height, data })
}

/// Subbands in CRX order: LLn, HLn, LHn, HHn, ... HL1, LH1, HH1.
pub(crate) fn reconstruct(bands: &[Band], width: usize, height: usize, levels: u8, edges: TileEdges) -> Result<Vec<i32>> {
    if levels > 3 || bands.len() != 1 + usize::from(levels) * 3 || width == 0 || height == 0 {
        return Err(corrupt());
    }
    let count = width.checked_mul(height).ok_or_else(corrupt)?;
    if count > crate::MAX_SAMPLES {
        return Err(RawError::Limit("CRX wavelet image size"));
    }
    let mut dimensions = vec![(width, height)];
    for _ in 0..levels {
        let &(w, h) = dimensions.last().ok_or_else(corrupt)?;
        dimensions.push((sizes(w, edges.left, edges.right).0, sizes(h, edges.top, edges.bottom).0));
    }
    let &(low_width, low_height) = dimensions.last().ok_or_else(corrupt)?;
    if !bands.first().is_some_and(|b| valid_band(b, low_width, low_height)) {
        return Err(corrupt());
    }
    let mut ll = bands.first().cloned().ok_or_else(corrupt)?;
    for step in 0..usize::from(levels) {
        let &(w, h) = dimensions.get(usize::from(levels) - step - 1).ok_or_else(corrupt)?;
        let index = 1 + step * 3;
        ll = inverse_level(
            &ll,
            bands.get(index).ok_or_else(corrupt)?,
            bands.get(index + 1).ok_or_else(corrupt)?,
            bands.get(index + 2).ok_or_else(corrupt)?,
            w,
            h,
            edges,
        )?;
    }
    if !valid_band(&ll, width, height) {
        return Err(corrupt());
    }
    Ok(ll.data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_lifting_preserves_signed_rounding_and_symmetric_ends() {
        let mut out = [0; 9];
        inverse_line(&[6, 11, 8, 6, 3], &[2, 2, -1, -2], &mut out, false, false).unwrap();
        assert_eq!(out, [5, 9, 10, 11, 8, 6, 7, 3, 4]);
        let mut out = [0; 2];
        inverse_line(&[25], &[20], &mut out, false, false).unwrap();
        assert_eq!(out, [15, 35]);
        let mut out = [0];
        inverse_line(&[-27], &[], &mut out, false, false).unwrap();
        assert_eq!(out, [-27]);
    }

    #[test]
    fn two_dimensional_subband_order() {
        let band = |value| Band { width: 1, height: 1, data: vec![value] };
        let out = reconstruct(&[band(25), band(10), band(20), band(0)], 2, 2, 1, TileEdges::default()).unwrap();
        assert_eq!(out, [10, 20, 30, 40]);
    }

    #[test]
    fn invalid_geometry_and_overflow_are_errors() {
        let mut out = [0; 2];
        assert!(inverse_line(&[], &[0], &mut out, false, false).is_err());
        assert!(inverse_line(&[i32::MAX], &[i32::MIN], &mut out, false, false).is_err());
        let b = Band { width: 1, height: 1, data: vec![0] };
        assert!(inverse_level(&b, &b, &b, &b, usize::MAX, 2, TileEdges::default()).is_err());
        assert!(reconstruct(&[b], 2, 2, 0, TileEdges::default()).is_err());
    }

    // Independent forward lifting in tests checks all odd/even boundary
    // combinations; hand-computed vectors above also establish sign/order.
    fn forward_line(input: &[i32]) -> (Vec<i32>, Vec<i32>) {
        let high: Vec<_> = (0..input.len() / 2)
            .map(|i| input[i * 2 + 1] - (input[i * 2] + input.get(i * 2 + 2).copied().unwrap_or(input[i * 2])).div_euclid(2))
            .collect();
        let low = input
            .iter()
            .step_by(2)
            .enumerate()
            .map(|(i, &v)| {
                if high.is_empty() {
                    v
                } else {
                    v + (high[i.saturating_sub(1)] + high.get(i).copied().unwrap_or(high[high.len() - 1]) + 2).div_euclid(4)
                }
            })
            .collect();
        (low, high)
    }

    #[test]
    fn every_boundary_parity_round_trips_signed_coefficients() {
        for width in 1..=17usize {
            for height in 1..=17usize {
                let input: Vec<i32> = (0..width * height).map(|i| ((i * 193 + i * i * 17) % 1001) as i32 - 500).collect();
                let (lw, hw, lhgt, hhgt) = (width.div_ceil(2), width / 2, height.div_ceil(2), height / 2);
                let mut vertical = vec![0; width * height];
                for x in 0..width {
                    let column: Vec<_> = (0..height).map(|y| input[y * width + x]).collect();
                    let (lo, hi) = forward_line(&column);
                    for (y, value) in lo.into_iter().chain(hi).enumerate() {
                        vertical[y * width + x] = value;
                    }
                }
                let mut bands = [
                    Band { width: lw, height: lhgt, data: vec![0; lw * lhgt] },
                    Band { width: hw, height: lhgt, data: vec![0; hw * lhgt] },
                    Band { width: lw, height: hhgt, data: vec![0; lw * hhgt] },
                    Band { width: hw, height: hhgt, data: vec![0; hw * hhgt] },
                ];
                for y in 0..height {
                    let (lo, hi) = forward_line(&vertical[y * width..(y + 1) * width]);
                    let (b, by) = if y < lhgt { (0, y) } else { (2, y - lhgt) };
                    bands[b].data[by * lw..(by + 1) * lw].copy_from_slice(&lo);
                    bands[b + 1].data[by * hw..(by + 1) * hw].copy_from_slice(&hi);
                }
                assert_eq!(reconstruct(&bands, width, height, 1, TileEdges::default()).unwrap(), input, "{width}x{height}");
            }
        }
    }
    // Independently generated from ITU-T T.800 integer 5/3 forward equations.
    // Original40x9 samples: ((x*23+y*37+x*y*7)%101)-50.
    // Tile width20 creates the same level3phase4 mismatch as R1001572.
    fn left_halo_fixture() -> Vec<Band> {
        vec![
            Band { width: 4, height: 2, data: vec![1, 1, -4, 1, 20, -2, -1, 2] },
            Band { width: 4, height: 2, data: vec![35, 21, -2, 19, -24, -10, -7, -2] },
            Band { width: 4, height: 1, data: vec![31, -16, -19, -7] },
            Band { width: 4, height: 1, data: vec![22, 9, 20, 18] },
            Band { width: 6, height: 3, data: vec![24, -34, -55, -21, 2, 7, 25, 5, -3, 14, 31, 14, 24, 37, 55, 19, -42, -41] },
            Band { width: 6, height: 2, data: vec![9, -21, 20, -4, 31, -6, -32, -3, 7, 31, 14, -11] },
            Band { width: 6, height: 2, data: vec![3, 62, 40, 50, 50, 51, -3, -62, -38, -45, 50, 35] },
            Band {
                width: 11,
                height: 5,
                data: vec![
                    13, -38, -12, 13, -50, -13, 13, 39, 25, -38, 38, -44, 6, 0, -44, -50, 57, 0, -6, 38, 19, -13, 26, 26, 39, -37, -62, -37, -37,
                    -25, -25, -44, -44, 6, -44, -12, 57, 39, -44, 13, 32, -18, 0, 57, -38, 13, 13, -13, 51, 13, -12, -38, 13, 13, -13,
                ],
            },
            Band {
                width: 11,
                height: 4,
                data: vec![
                    13, 39, 0, -25, 6, -6, 26, 0, -43, 20, -25, -50, 26, 7, -19, 0, 13, 32, -6, -25, 38, 32, 1, 26, -44, 19, 0, -12, -6, 45, -25, -6,
                    32, -37, 39, -12, 26, -6, 6, -25, 0, 39, -12, 26,
                ],
            },
            Band {
                width: 11,
                height: 4,
                data: vec![
                    26, -75, 76, 25, 0, -25, -75, 76, -50, -75, -25, 0, -101, -75, 0, 0, 51, 76, 101, 0, -51, -25, -101, 0, 25, 51, -50, 0, -25, 0,
                    101, 76, 51, -75, 26, -75, -25, 0, 25, 76, -75, 26, -75, -25,
                ],
            },
        ]
    }
    fn right_halo_fixture() -> Vec<Band> {
        vec![
            Band { width: 3, height: 2, data: vec![-12, 8, -5, 0, 4, -2] },
            Band { width: 3, height: 2, data: vec![-10, -5, -15, 8, 6, 5] },
            Band { width: 3, height: 1, data: vec![-10, -14, -10] },
            Band { width: 3, height: 1, data: vec![-17, -14, -25] },
            Band { width: 6, height: 3, data: vec![2, 7, -4, 7, 26, 38, 31, 14, 6, -12, -2, -22, -42, -41, -47, -7, 10, 44] },
            Band { width: 5, height: 2, data: vec![-6, 15, 31, -7, 12, -11, 6, 9, 5, -5] },
            Band { width: 6, height: 2, data: vec![50, 51, -46, -36, -58, -61, 50, 35, 69, 18, -36, -21] },
            Band {
                width: 11,
                height: 5,
                data: vec![
                    -38, 38, -13, 13, -38, 39, -25, -38, 63, 13, 49, 19, -13, -57, 70, 25, -6, -12, 32, 0, -44, -51, -44, -44, -50, -37, -37, -25, 1,
                    -31, -44, -50, -50, 0, 57, -37, -44, 13, 32, -12, 6, 57, -37, -36, 13, -13, 77, 13, -12, -38, -25, -25, -13, 77, -22,
                ],
            },
            Band {
                width: 10,
                height: 4,
                data: vec![
                    -25, 39, 0, -37, 25, -18, 45, -12, -37, 19, 32, -6, 13, 32, -6, 1, -25, 45, -6, 0, 32, 13, 0, -6, 45, -25, -6, 32, 13, 0, 26, 7,
                    19, -25, 0, 45, -50, 32, 7, 13,
                ],
            },
            Band {
                width: 11,
                height: 4,
                data: vec![
                    -75, -25, -25, 25, 25, 76, 51, -75, 25, 25, 51, -51, -25, 0, 51, 76, 101, 101, 0, -25, 0, 0, 76, 51, 0, 0, -25, 0, 101, 76, 51,
                    0, 0, -75, -25, 51, 25, 76, -75, 51, -50, -25, 51, 0,
                ],
            },
        ]
    }

    #[test]
    fn three_levels_keep_internal_tile_halos_and_local_phase() {
        for (tile, bands, edges) in [
            (0, left_halo_fixture(), TileEdges { right: true, ..Default::default() }),
            (1, right_halo_fixture(), TileEdges { left: true, ..Default::default() }),
        ] {
            let expected: Vec<i32> =
                (0..9).flat_map(|y| (tile * 20..tile * 20 + 20).map(move |x| ((x * 23 + y * 37 + x * y * 7) % 101) - 50)).collect();
            assert_eq!(reconstruct(&bands, 20, 9, 3, edges).unwrap(), expected);
            assert!(reconstruct(&bands, 20, 9, 3, TileEdges::default()).is_err());
        }
    }

    /// Shape-correct subbands in CRX order (LLn, HLn, LHn, HHn, …) filled with a
    /// deterministic pseudo-random pattern in ±30000, small enough that no lifting
    /// step can overflow. Values do not affect timing; shapes must satisfy every
    /// geometry check so the whole reconstruct path runs.
    fn pseudo_bands(width: usize, height: usize, levels: u8, edges: TileEdges, seed: u64) -> Vec<Band> {
        let mut state = seed | 1;
        let mut next = move || {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((state >> 33) % 60001) as i32 - 30000
        };
        let band =
            |width: usize, height: usize, next: &mut dyn FnMut() -> i32| Band { width, height, data: (0..width * height).map(|_| next()).collect() };
        let mut dimensions = vec![(width, height)];
        for _ in 0..levels {
            let &(w, h) = dimensions.last().expect("dimensions seeded with one entry");
            dimensions.push((sizes(w, edges.left, edges.right).0, sizes(h, edges.top, edges.bottom).0));
        }
        let mut bands = Vec::with_capacity(1 + usize::from(levels) * 3);
        let &(low_width, low_height) = dimensions.last().expect("levels > 0 pushed");
        bands.push(band(low_width, low_height, &mut next));
        for step in 0..usize::from(levels) {
            let &(w, h) = dimensions.get(usize::from(levels) - step - 1).expect("dimension per level");
            let (lw, hw) = sizes(w, edges.left, edges.right);
            let (lhgt, hhgt) = sizes(h, edges.top, edges.bottom);
            bands.push(band(hw, lhgt, &mut next));
            bands.push(band(lw, hhgt, &mut next));
            bands.push(band(hw, hhgt, &mut next));
        }
        bands
    }

    /// Ms per full 3-level reconstruct over representative plane sizes:
    /// full-frame, both odd parities, and single-tile halves with internal halos.
    /// Run with: cargo test -p lightcraft-raw --release wavelet_timing -- --ignored --nocapture
    #[test]
    #[ignore]
    fn wavelet_timing_representative_planes() {
        let cases = [
            ("4000x3000 frame", 4000, 3000, TileEdges::default()),
            ("3999x2999 odd", 3999, 2999, TileEdges::default()),
            ("4001x3001 odd", 4001, 3001, TileEdges::default()),
            ("2000x3000 left halo", 2000, 3000, TileEdges { left: true, ..Default::default() }),
            ("2000x3000 right halo", 2000, 3000, TileEdges { right: true, ..Default::default() }),
            ("2000x3000 middle halo", 2000, 3000, TileEdges { left: true, right: true, ..Default::default() }),
        ];
        for (label, width, height, edges) in cases {
            let bands = pseudo_bands(width, height, 3, edges, 0x5eed_c0de_1234_5678);
            let mut samples = [0f64; 3];
            for attempt in 0..3 {
                let start = std::time::Instant::now();
                let out = reconstruct(&bands, width, height, 3, edges).expect("pseudo bands are shape-valid");
                std::hint::black_box(&out);
                samples[attempt] = start.elapsed().as_secs_f64() * 1e3;
            }
            samples.sort_by(|a, b| a.total_cmp(b));
            let median = samples[1];
            let mp = (width * height) as f64 / 1e6;
            println!(
                "{label:24} median {median:7.2} ms ({:5.1} MP/s)  runs {:.2}/{:.2}/{:.2}",
                mp / (median / 1e3),
                samples[0],
                samples[1],
                samples[2]
            );
        }
    }
}
