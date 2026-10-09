//! Per-body CR3 colour fixtures: every corpus body's `ColorData` version, the XYZ → camera
//! matrices derived from the file's own white-balance planes (pinned to 4 decimals, green row
//! normalised at the 6504 K reference white), and the white-balance round trip that validates
//! them: a derived model must reproduce the file's as-shot multipliers. Skips cleanly when the
//! git-ignored corpus is absent (`cargo xtask corpus --download`).

use lightcraft_meta::cr3::parse_cr3;
use lightcraft_tiff::Tiff;

const COLOR_DATA: u16 = 0x4001;

fn corpus() -> std::path::PathBuf {
    std::env::var_os("LIGHTCRAFT_CORPUS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus"))
        .join("raw")
}

/// The file's `ColorData` version word (word 0 of the 0x4001 block; CR3 carries it in the timed
/// ExifInfo8 vendor metadata, with older bodies keeping it in the static maker note).
fn color_data_version(bytes: &[u8]) -> Option<u64> {
    let container = parse_cr3(bytes)?;
    let static_maker = container.cmt.get(2).and_then(|b| *b).and_then(|b| Tiff::parse(b).ok());
    let maker = static_maker.as_ref().and_then(|m| m.ifds.first());
    let timed = container
        .timed_exif(bytes)
        .into_iter()
        .filter(|b| b.tag == 0x927c)
        .filter_map(|b| Tiff::parse(b.data).ok())
        .find(|t| t.ifds.first().is_some_and(|m| m.contains(COLOR_DATA)));
    let ifd = timed.as_ref().and_then(|t| t.ifds.first()).or(maker)?;
    ifd.u64s(COLOR_DATA)?.first().copied()
}

/// One representative file per corpus body: `ColorData` version, as-shot white (Kelvin, tint),
/// and the derived matrices. 29 bodies, Canon's published generations v9 (16-19), v10 (32-33),
/// v11 (34, 48) and v12 (64-66) all covered; the R6 Mark III's version 66 follows the v12 layout.
const BODIES: &[(&str, u64, (i64, f64), Option<[[f64; 3]; 3]>, Option<[[f64; 3]; 3]>)] = &[
    // EOS Kiss M
    (
        "cr3-eos-kiss-m-img-2695.cr3",
        16,
        (5144, 1.0),
        Some([[2.8051, -2.0720, -0.0505], [-0.9426, 1.9559, -0.0383], [0.2459, -0.0321, 0.6145]]),
        Some([[2.9675, -2.0687, -0.2134], [-0.9676, 2.0330, -0.0854], [0.2906, -0.0079, 0.5500]]),
    ),
    // EOS M50
    (
        "cr3-canon-m50-craw.cr3",
        16,
        (4812, 1.9),
        Some([[2.8589, -2.1583, 0.0523], [-1.0492, 2.0521, -0.0319], [0.2695, -0.1332, 0.7040]]),
        Some([[2.9666, -2.0405, -0.2057], [-1.0035, 1.9827, -0.0096], [0.2903, -0.0074, 0.5142]]),
    ),
    // EOS R
    (
        "cr3-eos-r-canon-eos-r-craw-iso-100.cr3",
        17,
        (4831, 0.7),
        Some([[2.9385, -2.4277, 0.0699], [-1.0754, 2.2489, -0.1848], [0.3465, -0.1395, 0.5697]]),
        Some([[2.9639, -2.2306, -0.1760], [-0.9641, 2.1244, -0.1700], [0.3243, 0.0063, 0.4160]]),
    ),
    // EOS 200D II
    (
        "cr3-eos-200d-mark-ii-mg-3608.cr3",
        18,
        (5951, -1.6),
        Some([[2.5939, -2.1002, 0.0807], [-0.9694, 1.8336, 0.0939], [0.2061, -0.0980, 0.5198]]),
        Some([[2.8946, -2.1325, -0.1664], [-0.9651, 2.0263, -0.0816], [0.2620, -0.0390, 0.4189]]),
    ),
    // EOS 250D
    (
        "cr3-eos-250d-canon250d-craw-1-1.cr3",
        18,
        (5305, 2.0),
        Some([[2.8026, -2.2703, 0.1236], [-1.0347, 2.0964, -0.0840], [0.2555, -0.1576, 0.6423]]),
        Some([[2.9374, -2.1192, -0.2183], [-1.0089, 2.0085, -0.0279], [0.2688, -0.0354, 0.4084]]),
    ),
    // EOS RP
    (
        "cr3-eos-rp-canon-eos-rp-craw-iso-100-crop-nodual.cr3",
        18,
        (3869, 6.4),
        Some([[2.8040, -2.3409, 0.0598], [-1.1074, 2.1068, -0.0305], [0.2569, -0.1963, 0.6466]]),
        Some([[2.9676, -2.1798, -0.2295], [-0.9945, 2.0731, -0.0979], [0.2747, -0.0342, 0.4088]]),
    ),
    // EOS Rebel SL3
    (
        "cr3-eos-rebel-sl3-sedona2020-sl3-mg-1061.cr3",
        18,
        (5201, 2.1),
        Some([[2.7229, -2.1496, 0.0710], [-0.9470, 2.0234, -0.0947], [0.2566, -0.1203, 0.6227]]),
        Some([[2.9401, -2.1473, -0.1975], [-1.0135, 2.0246, -0.0383], [0.2934, -0.0188, 0.3868]]),
    ),
    // PowerShot SX70 HS
    (
        "cr3-powershot-sx70-hs-img-1000.cr3",
        18,
        (5416, 0.3),
        Some([[2.2844, -1.9486, 0.3118], [-1.1797, 1.6482, 0.4407], [0.1845, -0.3061, 0.6944]]),
        Some([[2.8581, -2.0188, -0.1282], [-0.9741, 1.9511, -0.0068], [0.1880, -0.1067, 0.5165]]),
    ),
    // EOS 850D
    (
        "cr3-eos-850d-img-1837.cr3",
        19,
        (6375, 0.7),
        Some([[2.6645, -2.1514, 0.0539], [-0.9745, 1.9023, 0.0371], [0.2333, -0.0798, 0.4759]]),
        Some([[2.9287, -2.0936, -0.2290], [-1.0065, 1.9921, -0.0153], [0.2576, -0.0449, 0.4232]]),
    ),
    // EOS 90D
    (
        "cr3-eos-90d-canon-90d-craw-iso-100.cr3",
        19,
        (6580, 0.4),
        Some([[2.6591, -2.0344, -0.0370], [-0.8969, 1.8632, 0.0049], [0.2046, -0.0553, 0.5199]]),
        Some([[2.9670, -2.0939, -0.2517], [-1.0159, 2.0035, -0.0174], [0.2638, -0.0379, 0.4529]]),
    ),
    // EOS M200
    (
        "cr3-canon-eos-m200-mg-2231.cr3",
        19,
        (5816, -0.3),
        Some([[2.7593, -1.9931, -0.0614], [-0.8985, 1.9008, -0.0272], [0.2220, -0.0176, 0.6063]]),
        Some([[2.9949, -2.0178, -0.2548], [-1.0064, 1.9843, -0.0085], [0.2856, -0.0099, 0.5398]]),
    ),
    // EOS M50 Mark II
    (
        "cr3-eos-m50-mark-ii-img-0051.cr3",
        19,
        (4055, 3.3),
        Some([[2.7991, -2.0758, -0.0190], [-0.9864, 2.0106, -0.0492], [0.2706, -0.1118, 0.7595]]),
        Some([[2.9900, -2.0537, -0.2410], [-1.0099, 2.0004, -0.0198], [0.3115, 0.0096, 0.5001]]),
    ),
    // EOS M6 Mark II
    (
        "cr3-eos-m6-mark-ii-img-craw-1-1.cr3",
        19,
        (4008, 6.6),
        Some([[2.5757, -1.9923, -0.0735], [-0.9969, 1.8637, 0.0908], [0.2147, -0.1686, 0.6286]]),
        Some([[2.9873, -2.1293, -0.2481], [-1.0087, 2.0438, -0.0595], [0.2732, -0.0347, 0.4628]]),
    ),
    // EOS R100
    (
        "cr3-canon-r100-craw.cr3",
        19,
        (4846, 1.4),
        Some([[2.7981, -2.3085, 0.1324], [-1.0749, 2.0957, -0.0486], [0.2773, -0.1559, 0.5641]]),
        Some([[2.8806, -2.1376, -0.1547], [-0.9775, 2.0096, -0.0560], [0.2785, -0.0253, 0.3959]]),
    ),
    // PowerShot G5 X Mark II
    (
        "cr3-powershot-g5-x-mark-ii-canon-powershot-g5-x-mark-ii-craw-1-1.cr3",
        19,
        (5985, 1.8),
        Some([[2.5564, -2.0227, -0.0513], [-0.7891, 1.9177, -0.1367], [0.2242, -0.0693, 0.4235]]),
        Some([[2.8914, -2.1470, -0.2406], [-0.9627, 2.0466, -0.1017], [0.2470, -0.0614, 0.3951]]),
    ),
    // PowerShot G7 X Mark III
    (
        "cr3-powershot-g7-x-mark-iii-img-0253.cr3",
        19,
        (5070, 1.1),
        Some([[2.1310, -1.7055, 0.0471], [-0.7794, 1.7186, 0.0324], [0.2243, -0.1501, 0.5191]]),
        Some([[2.9510, -2.2526, -0.1889], [-0.9414, 2.1510, -0.2133], [0.2640, -0.0495, 0.4042]]),
    ),
    // EOS-1D X Mark III
    (
        "cr3-eos-1d-x-mark-iii-canon-eos-1d-x-mark-iii-craw-iso-100-nocrop-nodual.cr3",
        32,
        (4286, 2.3),
        Some([[2.6561, -2.0635, 0.0289], [-0.9098, 1.9860, -0.0934], [0.2464, -0.1308, 0.6702]]),
        Some([[2.9544, -2.1259, -0.2278], [-0.9922, 2.0372, -0.0679], [0.2740, -0.0292, 0.4414]]),
    ),
    // EOS R5
    (
        "cr3-eos-r5-canon-eos-r5-craw-iso-100-crop-dual.cr3",
        33,
        (3789, 3.7),
        Some([[2.4553, -2.0214, 0.1034], [-0.9988, 1.8469, 0.1074], [0.2027, -0.1898, 0.5910]]),
        Some([[2.9548, -2.1716, -0.2144], [-1.0229, 2.0367, -0.0410], [0.2471, -0.0633, 0.3964]]),
    ),
    // EOS R6
    (
        "cr3-eos-r6-canon-eos-r6-craw-iso-100-crop-nodual.cr3",
        33,
        (4966, 1.2),
        Some([[2.8526, -2.3169, 0.1061], [-1.0592, 2.1627, -0.1220], [0.2975, -0.1608, 0.6030]]),
        Some([[2.9800, -2.1958, -0.1931], [-1.0076, 2.0816, -0.0942], [0.2893, -0.0314, 0.4294]]),
    ),
    // EOS R3
    (
        "cr3-eos-r3-c3a1200.cr3",
        34,
        (5753, -0.4),
        Some([[2.6535, -2.1242, 0.0243], [-0.9187, 1.9184, -0.0254], [0.1984, -0.1026, 0.4931]]),
        Some([[2.9630, -2.1084, -0.2783], [-1.0080, 2.0158, -0.0353], [0.2211, -0.0794, 0.4504]]),
    ),
    // EOS R10
    (
        "cr3-canon-eos-r10-img-6310.cr3",
        48,
        (5977, -1.0),
        Some([[2.5267, -2.0390, 0.1082], [-0.9748, 1.7734, 0.1523], [0.1912, -0.1145, 0.5315]]),
        Some([[2.9195, -2.1346, -0.1609], [-1.0095, 2.0049, -0.0242], [0.2495, -0.0564, 0.4286]]),
    ),
    // EOS R50
    (
        "cr3-eos-r50-canon-r50-iso100-craw-electronicshutter.cr3",
        48,
        (5274, -2.3),
        Some([[2.7000, -2.2037, 0.1317], [-1.0829, 1.9881, 0.0542], [0.2637, -0.1604, 0.5190]]),
        Some([[2.9209, -2.1515, -0.1495], [-1.0015, 2.0237, -0.0479], [0.2519, -0.0572, 0.4294]]),
    ),
    // EOS R6 Mark II
    (
        "cr3-eos-r6-mark-ii-canon-r6m2-img-0346-raw-normal-full.cr3",
        48,
        (6487, -0.3),
        Some([[2.7628, -2.1665, -0.0393], [-0.9222, 2.0180, -0.1113], [0.2563, -0.0627, 0.5245]]),
        Some([[2.9517, -2.0738, -0.2867], [-0.9666, 2.0286, -0.0823], [0.2486, -0.0489, 0.5188]]),
    ),
    // EOS R7
    (
        "cr3-eos-r7-443a0157.cr3",
        48,
        (4790, 0.6),
        Some([[2.6329, -2.0586, -0.0180], [-1.0267, 1.9156, 0.0703], [0.2234, -0.1730, 0.6522]]),
        Some([[2.9517, -2.1285, -0.2055], [-0.9884, 2.0359, -0.0701], [0.2644, -0.0385, 0.4699]]),
    ),
    // EOS R8
    (
        "cr3-canon-r8-craw.cr3",
        48,
        (4828, 1.6),
        Some([[2.8746, -2.3302, 0.0628], [-1.0425, 2.1794, -0.1513], [0.3014, -0.1495, 0.6145]]),
        Some([[2.9477, -2.1750, -0.1962], [-0.9749, 2.0734, -0.1151], [0.3048, -0.0040, 0.4231]]),
    ),
    // EOS R5 Mark II
    (
        "cr3-canon-eos-r5m2-raw.cr3",
        64,
        (4681, 2.8),
        Some([[2.7133, -2.1892, 0.0451], [-1.0223, 2.0389, -0.0434], [0.2606, -0.1652, 0.5982]]),
        Some([[2.9045, -2.1251, -0.1971], [-0.9433, 2.0504, -0.1218], [0.2601, -0.0436, 0.4753]]),
    ),
    // EOS R50 V
    (
        "cr3-canon-eos-r50-v-img-0034.cr3",
        65,
        (5606, -0.2),
        Some([[2.6305, -2.1080, 0.0880], [-0.9953, 1.8595, 0.0932], [0.2084, -0.1083, 0.5142]]),
        Some([[2.9660, -2.0765, -0.2607], [-1.0169, 1.9920, -0.0063], [0.2254, -0.0740, 0.4707]]),
    ),
    // PowerShot V1
    (
        "cr3-canon-powershot-v1-img-1225.cr3",
        65,
        (5635, 0.5),
        Some([[2.6807, -2.0729, -0.0165], [-0.9011, 1.9135, -0.0363], [0.2141, -0.0724, 0.5405]]),
        Some([[2.9814, -2.1107, -0.2597], [-0.9991, 2.0388, -0.0634], [0.2559, -0.0479, 0.4796]]),
    ),
    // EOS R6 Mark III
    (
        "cr3-canon-eos-r6-mark-iii-img-4241.cr3",
        66,
        (7285, 0.2),
        Some([[2.2597, -1.9464, 0.2840], [-0.9886, 1.7664, 0.1703], [0.2268, -0.1830, 0.5445]]),
        Some([[2.9524, -2.0878, -0.2684], [-1.0037, 1.9954, -0.0207], [0.2422, -0.0449, 0.4187]]),
    ),
];

/// The matrices round to these 4 decimals (the fit is deterministic).
fn round4(m: Option<lightcraft_raw::Mat3>) -> Option<[[f64; 3]; 3]> {
    m.map(|m| m.0.map(|r| r.map(|v| (v * 10000.0).round() / 10000.0)))
}

#[test]
fn per_body_matrix_version_and_wb_fixtures() {
    let max_wb_error = |image: &lightcraft_raw::RawImage| -> Option<f64> {
        let vendor = image.wb_multipliers?;
        let white = lightcraft_raw::color::as_shot_white_xy(image);
        let derived = lightcraft_raw::color::wb_multipliers(&image.color, white);
        Some(derived.iter().zip(vendor).map(|(d, v)| (d / f64::from(v) - 1.0).abs()).fold(0.0f64, f64::max))
    };
    let mut seen = 0;
    for (name, version, (temp, tint), m1, m2) in BODIES {
        let path = corpus().join(name);
        let Ok(bytes) = std::fs::read(&path) else {
            eprintln!("skip: {} absent", path.display());
            continue;
        };
        let image = lightcraft_raw::decode(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(color_data_version(&bytes), Some(*version), "{name}: ColorData version");
        assert!(lightcraft_raw::color::has_matrix(&image.color), "{name}: derives a colour matrix");
        assert_eq!(round4(image.color.color_matrix[0]), *m1, "{name}: matrix 1");
        assert_eq!(round4(image.color.color_matrix[1]), *m2, "{name}: matrix 2");
        assert!(image.wb_multipliers.is_some(), "{name}: vendor white balance present");
        let error = max_wb_error(&image).unwrap_or(f64::NAN);
        assert!(error <= 1e-3, "{name}: derived white balance off by {error}");
        let white = lightcraft_raw::color::as_shot_white_xy(&image);
        let (t, i) = lightcraft_color::cct::xy_to_temp_tint(white);
        assert!((t - *temp as f64).abs() <= 2.0 && (i - *tint).abs() <= 0.6, "{name}: as-shot ({t}, {i}) vs pinned ({temp}, {tint})");
        seen += 1;
    }
    if seen > 0 {
        assert_eq!(seen, BODIES.len(), "every corpus body must decode");
    }
}

/// The whole corpus derives validated matrices, each reproducing its file's as-shot multipliers.
#[test]
fn whole_corpus_derives_matrices_and_round_trips_wb() {
    let Ok(entries) = std::fs::read_dir(corpus()) else {
        eprintln!("skip: no corpus");
        return;
    };
    let mut files: Vec<_> = entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "cr3")).collect();
    files.sort();
    let mut checked = 0;
    for path in files {
        let Ok(bytes) = std::fs::read(&path) else { continue };
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let image = lightcraft_raw::decode(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(lightcraft_raw::color::has_matrix(&image.color), "{name}: derives a colour matrix");
        let vendor = image.wb_multipliers.expect("as-shot multipliers");
        let derived = lightcraft_raw::color::wb_multipliers(&image.color, lightcraft_raw::color::as_shot_white_xy(&image));
        let error = derived.iter().zip(vendor).map(|(d, v)| (d / f64::from(v) - 1.0).abs()).fold(0.0f64, f64::max);
        assert!(error <= 1e-3, "{name}: derived white balance off by {error}");
        checked += 1;
    }
    if checked > 0 {
        assert_eq!(checked, 110, "every corpus CR3 must decode");
    }
}
