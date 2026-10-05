//! Public CLI integration with real native raster I/O and serialized SVG rendering.
use coord2svg::{
    output::Report,
    raster,
    sources::{self, Region},
    terrain::Grid,
};
use resvg::{
    tiny_skia::{Pixmap, Transform},
    usvg,
};
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
fn region() -> Region {
    Region {
        latitude: 21.058334,
        longitude: -157.374151,
        radius_m: 1000.,
    }
}
fn fixture(path: &Path) -> Grid {
    let grid = Grid::analytic(33, |x, y| 100. * x + 20. * y).unwrap();
    raster::save_grid(path, &region(), &grid, "EGM2008").unwrap();
    grid
}
fn cli(input: &Path, out: &Path, extra: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_coord2svg"))
        .args([
            "--lat",
            "21.058334",
            "--lon",
            "-157.374151",
            "--radius",
            "1km",
            "--grid-size",
            "33",
            "--levels",
            "16",
            "--source",
            "local",
            "--input-datum",
            "EGM2008",
            "--offline",
            "--width",
            "512",
            "--input",
        ])
        .arg(input)
        .arg("--out")
        .arg(out)
        .args(extra)
        .output()
        .unwrap()
}
fn report(path: &Path) -> Report {
    serde_json::from_slice(&fs::read(path.with_extension("json")).unwrap()).unwrap()
}
fn render(svg: &str) -> Pixmap {
    let tree = usvg::Tree::from_str(svg, &usvg::Options::default()).unwrap();
    let mut p = Pixmap::new(512, 512).unwrap();
    resvg::render(&tree, Transform::default(), &mut p.as_mut());
    p
}
#[test]
fn local_raster_cli_report_and_rendered_filtered_svg_agree() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("日本語 elevation fixture.tif");
    let out = dir.path().join("selected terrain.svg");
    fixture(&input);
    let result = cli(
        &input,
        &out,
        &[
            "--min-elevation",
            "0ft",
            "--max-elevation",
            "250ft",
            "--lowest-fill",
            "#888888",
            "--highest-fill",
            "#888888",
            "--no-stroke",
            "--min-coverage",
            "0.99",
        ],
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let r = report(&out);
    assert_eq!(r.schema_version, 3);
    assert_eq!(r.vertical_datum, "EGM2008");
    assert_eq!(r.sources.len(), 1);
    assert_eq!(r.sources[0].sha256, sources::checksum(&input).unwrap().0);
    assert_eq!(r.sources[0].contributing_grid_nodes, 33 * 33);
    assert_eq!(r.accuracy.minimum_elevation_m, 0.);
    assert!((r.accuracy.maximum_elevation_m - 76.2).abs() < 1e-10);
    assert!(r.accuracy.coverage_fraction > 0.99 && r.accuracy.retained_fraction < 0.5);
    assert_eq!(r.accuracy.audit_missing, 0);
    assert!(r.accuracy.audit_rmse_m.unwrap() <= r.accuracy.quantization_bound_m);
    let svg = fs::read_to_string(out).unwrap();
    let xml = roxmltree::Document::parse(&svg).unwrap();
    assert!(!xml.descendants().any(|n| n.has_tag_name("polygon")));
    for group in xml
        .descendants()
        .filter(|n| n.attribute("data-elevation-m").is_some())
    {
        assert_eq!(group.attribute("stroke"), Some("none"));
        assert_eq!(group.attribute("stroke-width"), Some("0"));
        assert_eq!(
            group.children().filter(|n| n.has_tag_name("path")).count(),
            1
        );
    }
    let p = render(&svg);
    for y in 32..480 {
        for x in 32..480 {
            let nx = (x as f64 + 0.5) / 256. - 1.;
            let ny = 1. - (y as f64 + 0.5) / 256.;
            let z = 100. * nx + 20. * ny;
            if nx * nx + ny * ny > 0.75 * 0.75 {
                continue;
            }
            let pixel = p.pixel(x, y).unwrap();
            if (2. ..74.).contains(&z) {
                assert_eq!(
                    (pixel.red(), pixel.green(), pixel.blue(), pixel.alpha()),
                    (136, 136, 136, 255),
                    "paint mismatch at {x},{y}"
                );
            }
            if z < -2. || z > 78.2 {
                assert_eq!(pixel.alpha(), 0, "excluded pixel at {x},{y}");
            }
        }
    }
}
#[test]
fn saved_working_raster_remains_unfiltered_and_can_be_reused() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.tif");
    let out = dir.path().join("land.svg");
    let saved = dir.path().join("working grid.tif");
    let expected = fixture(&input);
    let result = cli(
        &input,
        &out,
        &[
            "--min-elevation",
            "0m",
            "--save-grid",
            saved.to_str().unwrap(),
        ],
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let r = report(&out);
    let (sampled, _) =
        raster::sample_files(&region(), 33, &[(saved.clone(), r.sources[0].clone())]).unwrap();
    for (actual, expected) in sampled.values.iter().zip(&expected.values) {
        assert!((actual - expected).abs() < 1e-7);
    }
    assert!(
        sampled.values.iter().any(|z| *z < 0.),
        "filter mutated saved terrain"
    );
    let reused = dir.path().join("reused.svg");
    let result = cli(&saved, &reused, &["--min-elevation", "0m"]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let reused = report(&reused);
    assert_eq!(reused.accuracy.audit_missing, 0);
    assert!((r.accuracy.maximum_elevation_m - reused.accuracy.maximum_elevation_m).abs() < 1e-7);
    assert!((r.accuracy.retained_fraction - reused.accuracy.retained_fraction).abs() < 1e-8);
}
#[test]
fn empty_selection_fails_without_writing_outputs_or_saved_grid() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.tif");
    let out = dir.path().join("empty.svg");
    let saved = dir.path().join("unused.tif");
    fixture(&input);
    let result = cli(
        &input,
        &out,
        &[
            "--min-elevation",
            "1mi",
            "--save-grid",
            saved.to_str().unwrap(),
        ],
    );
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("no terrain remains"));
    assert!(!out.exists());
    assert!(!out.with_extension("json").exists());
    assert!(!saved.exists());
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
}
#[test]
fn cli_patterns_specific_overrides_and_disabled_paint_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.tif");
    let out = dir.path().join("lines.svg");
    fixture(&input);
    let result = cli(
        &input,
        &out,
        &[
            "--min-elevation",
            "0",
            "--no-fill",
            "--stroke-fill",
            "#333333",
            "--stroke-width",
            "0.2px",
            "--every-n-stroke-fill",
            "0ft,5,#000000",
            "--every-n-stroke-width",
            "0ft,5,2px",
            "--specific-fill",
            "10m,#ff0000",
            "--specific-stroke-fill",
            "0ft,#00ff00",
            "--specific-stroke-width",
            "0ft,4px",
        ],
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let r = report(&out);
    assert!(r.levels.iter().all(|l| l.fill == "none"));
    let first = &r.levels[0];
    assert_eq!(first.stroke_fill, "#00ff00");
    assert_eq!(first.stroke_width_px, 4.);
    for l in &r.levels[1..] {
        let emphasized = l.base_level_index.is_some_and(|i| i.is_multiple_of(5));
        assert_eq!(l.stroke_width_px, if emphasized { 2. } else { 0.2 });
    }
    assert!(
        r.levels
            .iter()
            .any(|l| l.elevation_m == 10. && l.base_level_index.is_none())
    );
    let svg = fs::read_to_string(out).unwrap();
    let doc = roxmltree::Document::parse(&svg).unwrap();
    let embedded: Report = serde_json::from_str(
        doc.descendants()
            .find(|n| n.has_tag_name("metadata"))
            .unwrap()
            .text()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(embedded.levels.len(), r.levels.len());
    for group in doc
        .descendants()
        .filter(|n| n.attribute("data-elevation-m").is_some())
    {
        assert_eq!(group.attribute("fill"), Some("none"));
        let z: f64 = group
            .attribute("data-elevation-m")
            .unwrap()
            .parse()
            .unwrap();
        let level = r.levels.iter().find(|l| l.elevation_m == z).unwrap();
        assert_eq!(group.attribute("stroke"), Some(level.stroke_fill.as_str()));
        assert_eq!(
            group
                .attribute("stroke-width")
                .unwrap()
                .parse::<f64>()
                .unwrap(),
            level.stroke_width_px
        );
    }
}
