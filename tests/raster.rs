use coord2svg::{
    raster::{self, RasterSpec},
    sources::{Asset, Product, Region},
    terrain::Grid,
};
use std::path::Path;

// Constant-height bilinear resampling can differ by a few floating-point ULPs
// across GDAL builds/CPUs. This is a roundoff allowance in metres, not a source
// accuracy tolerance; missing values and tile selection must still fail.
fn assert_constant_height(actual: f64, expected: f64) {
    const TOLERANCE_M: f64 = 1e-12;
    assert!(
        actual.is_finite() && (actual - expected).abs() <= TOLERANCE_M,
        "expected {expected} m within {TOLERANCE_M} m, got {actual} m"
    );
}

fn region() -> Region {
    Region {
        latitude: 21.,
        longitude: -157.,
        radius_m: 1000.,
    }
}
fn asset() -> Asset {
    Asset {
        product: Product {
            id: "fixture".into(),
            provider: "test".into(),
            url: "local".into(),
            attribution: "test".into(),
            vertical_datum: "EGM2008".into(),
            native_spacing_arcseconds: None,
            surface_type: "test".into(),
            published_vertical_accuracy: None,
        },
        sha256: "fixture".into(),
        bytes: 0,
        contributing_grid_nodes: 0,
    }
}
fn write(
    path: &Path,
    region: &Region,
    size: usize,
    values: &[f64],
    scale: f64,
    offset: f64,
    unit: &str,
) {
    let step = 2. * region.radius_m / (size - 1) as f64;
    raster::write_raster(
        path,
        &RasterSpec {
            projection: &region.proj4(),
            width: size,
            height: size,
            affine: [
                -region.radius_m - step / 2.,
                step,
                0.,
                region.radius_m + step / 2.,
                0.,
                -step,
            ],
            values,
            nodata: -9999.,
            scale,
            offset,
            datum: "EGM2008",
            unit,
        },
    )
    .unwrap();
}
#[test]
fn saved_grid_roundtrips() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("日本語 fixture.tif");
    let reg = region();
    let g = Grid::analytic(17, |x, y| 100. * x + 50. * y - 1000.).unwrap();
    raster::save_grid(&p, &reg, &g, "EGM2008").unwrap();
    let (actual, assets) = raster::sample_files(&reg, 17, &[(p, asset())]).unwrap();
    for (a, b) in actual.values.iter().zip(&g.values) {
        assert!((a - b).abs() < 1e-7, "{a} vs {b}");
    }
    assert_eq!(assets[0].contributing_grid_nodes, 289);
}
#[test]
fn raster_scale_and_offset_are_applied() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("scaled.tif");
    let reg = region();
    write(&p, &reg, 9, &vec![10.; 81], 2., -100., "m");
    let (g, _) = raster::sample_files(&reg, 9, &[(p, asset())]).unwrap();
    assert!(g.values.iter().all(|z| (*z + 80.).abs() < 1e-7));
}
#[test]
fn nodata_does_not_become_elevation() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("missing.tif");
    let reg = region();
    let mut v = vec![10.; 81];
    v[40] = -9999.;
    write(&p, &reg, 9, &v, 1., 0., "m");
    let (g, _) = raster::sample_files(&reg, 9, &[(p, asset())]).unwrap();
    assert!(!g.values[40].is_finite());
}
#[test]
fn later_valid_source_overlays_and_nodata_preserves_fallback() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.tif");
    let b = dir.path().join("b.tif");
    let reg = region();
    write(&a, &reg, 9, &vec![10.; 81], 1., 0., "m");
    let mut values = vec![20.; 81];
    values[40] = -9999.;
    write(&b, &reg, 9, &values, 1., 0., "m");
    let (g, _) = raster::sample_files(&reg, 9, &[(a, asset()), (b, asset())]).unwrap();
    assert_constant_height(g.values[40], 10.);
    assert_constant_height(g.values[20], 20.);
}
#[test]
fn unsupported_elevation_units_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("feet.tif");
    let reg = region();
    write(&p, &reg, 9, &vec![1.; 81], 1., 0., "ft");
    assert!(
        raster::sample_files(&reg, 9, &[(p, asset())])
            .unwrap_err()
            .to_string()
            .contains("metres")
    );
}
#[test]
fn unreadable_raster_is_error() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("bad.tif");
    std::fs::write(&p, b"not a raster").unwrap();
    assert!(raster::validate(&p).is_err());
}

#[test]
fn declared_datum_must_match_embedded_reference() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("datum.tif");
    let reg = region();
    write(&p, &reg, 9, &vec![10.; 81], 1., 0., "m");
    let mut a = asset();
    a.product.vertical_datum = "NAVD88".into();
    assert!(
        raster::sample_files(&reg, 9, &[(p, a)])
            .unwrap_err()
            .to_string()
            .contains("datum conflicts")
    );
}
#[test]
fn source_contribution_counts_reflect_final_owners() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.tif");
    let b = dir.path().join("b.tif");
    let reg = region();
    write(&a, &reg, 9, &vec![10.; 81], 1., 0., "m");
    write(&b, &reg, 9, &vec![20.; 81], 1., 0., "m");
    let (_, sources) = raster::sample_files(&reg, 9, &[(a, asset()), (b, asset())]).unwrap();
    assert_eq!(sources[0].contributing_grid_nodes, 0);
    assert_eq!(sources[1].contributing_grid_nodes, 81);
}
#[test]
fn date_line_tiles_have_no_false_gap() {
    let dir = tempfile::tempdir().unwrap();
    let mut files = vec![];
    for (west, height, name) in [(179., 10., "east"), (-180., 20., "west")] {
        let p = dir.path().join(format!("{name}.tif"));
        let values = vec![height; 10000];
        raster::write_raster(
            &p,
            &RasterSpec {
                projection: "EPSG:4326",
                width: 100,
                height: 100,
                affine: [west, 0.01, 0., 1., 0., -0.01],
                values: &values,
                nodata: -9999.,
                scale: 1.,
                offset: 0.,
                datum: "EGM2008",
                unit: "m",
            },
        )
        .unwrap();
        files.push((p, asset()));
    }
    let reg = Region {
        latitude: 0.5,
        longitude: 179.99,
        radius_m: 20_000.,
    };
    let (g, _) = raster::sample_files(&reg, 41, &files).unwrap();
    assert!(g.values.iter().all(|z| z.is_finite()));
    // Check every row on both sides, not just the central row. Keep full-grid
    // finiteness strict so the tolerance cannot conceal an antimeridian gap.
    for row in 0..41 {
        assert_constant_height(g.values[row * 41 + 10], 10.);
        assert_constant_height(g.values[row * 41 + 30], 20.);
    }
}

#[test]
fn constant_height_check_accepts_roundoff_but_rejects_bad_samples() {
    assert_constant_height(20.000000000000004, 20.);
    assert_constant_height(9.999999999999998, 10.);
    for actual in [f64::NAN, f64::INFINITY, 0., 10., 20.000001] {
        assert!(
            std::panic::catch_unwind(|| assert_constant_height(actual, 20.)).is_err(),
            "invalid sample {actual} was accepted"
        );
    }
}
