use coord2svg::{
    geometry,
    terrain::{self, ElevationFilter, Grid, Index, Settings},
};
use std::process::Command;

fn settings(minimum_m: Option<f64>, maximum_m: Option<f64>) -> Settings {
    Settings {
        elevation_filter: ElevationFilter {
            minimum_m,
            maximum_m,
        },
        level_count: 2,
        boundary_tolerance_m: 0.2,
        audit_samples: 4096,
        ..Default::default()
    }
}

#[test]
fn exact_limits_clip_geometry_despite_coarse_bands() {
    let grid = Grid::analytic(17, |x, _| 1000. * x).unwrap();
    for (min, max) in [
        (Some(0.), None),
        (None, Some(250.)),
        (Some(-250.), Some(250.)),
    ] {
        for merge in [false, true] {
            let mut s = settings(min, max);
            s.merge = merge;
            let t = terrain::build(&grid, &s).unwrap();
            let index = Index::new(&t);
            for i in -90..=90 {
                let x = i as f64 / 100.;
                assert_eq!(
                    index.elevation(x, 0.).is_some(),
                    s.elevation_filter.includes(x * 1000.)
                );
            }
            for p in t.layers.iter().flat_map(|l| &l.polygons) {
                assert!(geometry::is_convex(p));
                for v in p {
                    let z = 1000. * v.x;
                    assert!(min.is_none_or(|m| z >= m - 1e-6));
                    assert!(max.is_none_or(|m| z <= m + 1e-6));
                }
            }
            assert!(t.accuracy.coverage_fraction > 0.999);
            assert!(t.accuracy.retained_fraction < 0.7);
            assert!(t.accuracy.audit_elevation_excluded > 0);
            assert_eq!(t.accuracy.audit_missing, 0);
            assert_eq!(t.layers.first().unwrap().level.gray, 255);
            assert_eq!(t.layers.last().unwrap().level.gray, 0);
            assert!(
                t.accuracy.audit_max_abs_error_m.unwrap() <= t.accuracy.quantization_bound_m + 1e-6
            );
            let counts = t.accuracy.audit_samples
                + t.accuracy.audit_missing
                + t.accuracy.audit_elevation_excluded
                + t.accuracy.audit_source_nodata
                + t.accuracy.audit_boundary_excluded;
            assert_eq!(counts, t.accuracy.audit_attempts);
        }
    }
}

#[test]
fn filter_preserves_holes_and_islands() {
    let grid = Grid::analytic(33, |x, y| 1000. * (x * x + y * y)).unwrap();
    let t = terrain::build(&grid, &settings(Some(100.), Some(500.))).unwrap();
    let index = Index::new(&t);
    assert!(index.elevation(0., 0.).is_none());
    assert!(index.elevation(0.5, 0.).is_some());
    assert!(index.elevation(0.9, 0.).is_none());
    assert!(
        t.layers
            .iter()
            .flat_map(|l| &l.polygons)
            .all(|p| geometry::is_convex(p))
    );
}

#[test]
fn flat_terrain_at_inclusive_bounds_remains() {
    let grid = Grid::analytic(9, |_, _| 42.).unwrap();
    for s in [
        settings(Some(42.), None),
        settings(None, Some(42.)),
        settings(Some(42.), Some(42.)),
    ] {
        let t = terrain::build(&grid, &s).unwrap();
        assert_eq!(t.layers.len(), 1);
        assert_eq!(t.layers[0].level.elevation_m, 42.);
        assert_eq!(t.accuracy.audit_elevation_excluded, 0);
        assert_eq!(t.accuracy.retained_fraction, t.accuracy.coverage_fraction);
    }
}

#[test]
fn invalid_or_empty_ranges_fail_clearly() {
    let grid = Grid::analytic(9, |x, _| x * 100.).unwrap();
    for s in [
        settings(Some(f64::NAN), None),
        settings(None, Some(f64::INFINITY)),
        settings(Some(10.), Some(0.)),
    ] {
        assert!(terrain::build(&grid, &s).is_err());
    }
    for s in [
        settings(Some(101.), None),
        settings(None, Some(-101.)),
        settings(Some(0.), Some(0.)),
    ] {
        assert!(
            terrain::build(&grid, &s)
                .unwrap_err()
                .to_string()
                .contains("no terrain remains")
        );
    }
}

#[test]
fn fixed_color_range_is_independent_and_area_is_consistent() {
    let grid = Grid::analytic(17, |x, _| 1000. * x).unwrap();
    let mut s = settings(Some(0.), Some(500.));
    s.color_range = Some((-1000., 1000.));
    let t = terrain::build(&grid, &s).unwrap();
    assert_eq!(t.layers[0].level.gray, 128);
    assert_eq!(t.layers[1].level.gray, 64);
    let area: f64 = t.layers.iter().map(|l| l.level.area_m2).sum();
    assert!((area - t.accuracy.retained_area_m2).abs() < 0.01);
    assert!(t.accuracy.rounding_area_change_m2 < 0.01);
}

fn dry_run(extra: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_coord2svg"))
        .args([
            "--lat",
            "20.9",
            "--lon",
            "-157.4",
            "--radius",
            "500km",
            "--dry-run",
        ])
        .args(extra)
        .output()
        .unwrap()
}

#[test]
fn cli_units_and_geographic_fetch_plan() {
    let baseline: serde_json::Value = serde_json::from_slice(&dry_run(&[]).stdout).unwrap();
    for unit in ["10000ft", "10000 FT", "3048m", "3.048km", "3048"] {
        let result = dry_run(&["--min-elevation", "-5000m", "--max-elevation", unit]);
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let plan: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
        assert_eq!(plan["elevation_filter"]["minimum_m"], -5000.);
        assert_eq!(plan["elevation_filter"]["maximum_m"], 3048.);
        assert_eq!(plan["products"], baseline["products"]);
    }
    for args in [
        ["--min-elevation", "nan"],
        ["--max-elevation", "inf"],
        ["--min-elevation", "1e309ft"],
        ["--max-elevation", "100yd"],
    ] {
        assert!(!dry_run(&args).status.success());
    }
    assert!(
        !dry_run(&["--min-elevation", "100", "--max-elevation", "0"])
            .status
            .success()
    );
}

#[test]
fn cli_report_svg_and_coverage_threshold_with_filter() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("land.svg");
    let result = Command::new(env!("CARGO_BIN_EXE_coord2svg"))
        .args([
            "--lat",
            "20.9",
            "--lon",
            "-157.4",
            "--radius",
            "500km",
            "--demo",
            "--grid-size",
            "33",
            "--levels",
            "8",
            "--min-elevation",
            "0",
            "--max-elevation",
            "10000ft",
            "--min-coverage",
            "0.99",
            "--out",
        ])
        .arg(&path)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path.with_extension("json")).unwrap()).unwrap();
    assert_eq!(report["accuracy"]["elevation_filter"]["maximum_m"], 3048.);
    assert!(report["accuracy"]["retained_fraction"].as_f64().unwrap() < 0.5);
    let svg = std::fs::read_to_string(path).unwrap();
    let doc = roxmltree::Document::parse(&svg).unwrap();
    for group in doc
        .descendants()
        .filter(|n| n.attribute("data-elevation-m").is_some())
    {
        let z: f64 = group
            .attribute("data-elevation-m")
            .unwrap()
            .parse()
            .unwrap();
        assert!((0. ..=3048.).contains(&z));
    }
}
