use coord2svg::{
    geometry::Vertex,
    output::{self, Report},
    sources::Region,
    terrain::{self, Grid, Index, Settings},
};
use std::{
    fs,
    process::{Command, Output},
};
fn cmd(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_coord2svg"))
        .args(args)
        .output()
        .unwrap()
}
fn fixture() -> (terrain::Terrain, Report) {
    let t = terrain::build(
        &Grid::analytic(17, |x, y| 100. * x + 20. * y).unwrap(),
        &Settings {
            level_count: 9,
            ..Default::default()
        },
    )
    .unwrap();
    let r = Report::new(
        Region {
            latitude: 0.,
            longitude: 0.,
            radius_m: 1000.,
        },
        "EGM2008".into(),
        vec![],
        &t,
    );
    (t, r)
}
#[test]
fn svg_structure_and_exported_geometry_are_valid() {
    let (mut t, r) = fixture();
    assert!(Index::new(&t).elevation(0.4, 0.).is_some());
    let mut bytes = vec![];
    output::write_svg(&mut bytes, &t, &r, 1024).unwrap();
    let text = String::from_utf8(bytes).unwrap();
    let doc = roxmltree::Document::parse(&text).unwrap();
    let embedded: Report = serde_json::from_str(
        doc.descendants()
            .find(|n| n.has_tag_name("metadata"))
            .unwrap()
            .text()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(embedded.accuracy.polygon_count, t.accuracy.polygon_count);
    let mut last = f64::NEG_INFINITY;
    let mut count = 0;
    // Reconstruct union geometry from serialized paths, not the in-memory mesh.
    for layer in &mut t.layers {
        layer.footprint.clear();
    }
    for node in doc
        .descendants()
        .filter(|n| n.attribute("data-elevation-m").is_some())
    {
        let z: f64 = node.attribute("data-elevation-m").unwrap().parse().unwrap();
        assert!(z > last);
        last = z;
        assert!(!node.children().any(|n| n.has_tag_name("polygon")));
        let paths: Vec<_> = node.children().filter(|n| n.has_tag_name("path")).collect();
        assert_eq!(paths.len(), 1);
        let path = paths[0];
        assert_eq!(path.attribute("fill-rule"), Some("nonzero"));
        let mut tokens = path.attribute("d").unwrap().split_whitespace();
        let mut rings = vec![];
        let mut ring = vec![];
        while let Some(command) = tokens.next() {
            match command {
                "M" | "L" => {
                    let (x, y) = tokens.next().unwrap().split_once(',').unwrap();
                    ring.push(Vertex::new(
                        x.parse().unwrap(),
                        -y.parse::<f64>().unwrap(),
                        z,
                    ));
                }
                "Z" => {
                    assert!(ring.len() >= 3);
                    rings.push(std::mem::take(&mut ring));
                }
                _ => panic!("unexpected SVG command {command}"),
            }
        }
        assert!(ring.is_empty());
        count += 1;
        let layer = t
            .layers
            .iter_mut()
            .find(|l| l.level.elevation_m == z)
            .unwrap();
        assert_eq!(rings.len(), layer.level.footprint_ring_count);
        layer.footprint.push(coord2svg::union::Shape { rings });
    }
    assert_eq!(count, t.layers.len());
    let idx = Index::new(&t);
    for x in -8..=8 {
        for y in -8..=8 {
            let (x, y) = (x as f64 / 10., y as f64 / 10.);
            if x * x + y * y < 0.9 {
                let actual = idx
                    .elevation(x, y)
                    .unwrap_or_else(|| panic!("SVG gap at {x},{y}"));
                assert!(
                    (actual - (100. * x + 20. * y)).abs() <= t.accuracy.quantization_bound_m + 1e-8
                );
            }
        }
    }
}
#[test]
fn metadata_is_escaped_and_roundtrips() {
    let (t, mut r) = fixture();
    r.vertical_datum = "a<&\"b".into();
    let mut bytes = vec![];
    output::write_svg(&mut bytes, &t, &r, 128).unwrap();
    let text = String::from_utf8(bytes).unwrap();
    let doc = roxmltree::Document::parse(&text).unwrap();
    let metadata = doc
        .descendants()
        .find(|n| n.has_tag_name("metadata"))
        .unwrap();
    let result: Report = serde_json::from_str(metadata.text().unwrap()).unwrap();
    assert_eq!(result.vertical_datum, r.vertical_datum);
}
#[test]
fn save_prevents_clobber_and_force_replaces_pair() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("map.svg");
    let (t, r) = fixture();
    output::save(&p, &t, &r, 128, false).unwrap();
    let first = fs::read(&p).unwrap();
    assert!(output::save(&p, &t, &r, 256, false).is_err());
    assert_eq!(fs::read(&p).unwrap(), first);
    output::save(&p, &t, &r, 256, true).unwrap();
    assert_ne!(fs::read(&p).unwrap(), first);
}
#[test]
fn cli_demo_creates_svg_and_report() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("demo.svg");
    let o = cmd(&[
        "--lat",
        "21",
        "--lon",
        "-157",
        "--radius",
        "1km",
        "--demo",
        "--grid-size",
        "17",
        "--out",
        p.to_str().unwrap(),
    ]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(p.exists());
    assert!(p.with_extension("json").exists());
    let r: Report = serde_json::from_slice(&fs::read(p.with_extension("json")).unwrap()).unwrap();
    assert_eq!(r.request.radius_m, 1000.);
    assert!(r.accuracy.polygon_count > 0);
}
#[test]
fn cli_dry_run_has_no_side_effects() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("new/map.svg");
    let cache = dir.path().join("cache");
    let o = cmd(&[
        "--lat",
        "21",
        "--lon",
        "-157",
        "--radius",
        "1000km",
        "--dry-run",
        "--out",
        p.to_str().unwrap(),
        "--cache-dir",
        cache.to_str().unwrap(),
    ]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["request"]["radius_m"], 1_000_000.);
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
}
#[test]
fn cli_bad_arguments_fail_before_download() {
    for (flag, value) in [
        ("--radius", "0km"),
        ("--radius", "1001km"),
        ("--radius", "NaN"),
        ("--lat", "91"),
        ("--lon", "181"),
        ("--grid-size", "2"),
        ("--levels", "1"),
        ("--resolution", "0.01m"),
        ("--boundary-tolerance", "2000m"),
    ] {
        let mut args = vec![
            "--lat",
            "21",
            "--lon",
            "-157",
            "--radius",
            "1km",
            "--demo",
            "--dry-run",
        ];
        if let Some(i) = args.iter().position(|&s| s == flag) {
            args[i + 1] = value;
        } else {
            args.extend([flag, value]);
        }
        let o = cmd(&args);
        assert!(!o.status.success(), "accepted {flag} {value}");
    }
}
#[test]
fn local_datum_is_required_and_mixing_rejected() {
    let o = cmd(&[
        "--lat",
        "21",
        "--lon",
        "-157",
        "--radius",
        "1km",
        "--input",
        "absent.tif",
        "--dry-run",
    ]);
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("input-datum"));
    let o = cmd(&[
        "--lat",
        "21",
        "--lon",
        "-157",
        "--radius",
        "1km",
        "--source",
        "etopo",
        "--input",
        "absent.tif",
        "--input-datum",
        "NAVD88",
        "--dry-run",
    ]);
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("vertical datum conversion"));
}
#[test]
fn output_determinism() {
    let (t, r) = fixture();
    let mut a = vec![];
    let mut b = vec![];
    output::write_svg(&mut a, &t, &r, 128).unwrap();
    output::write_svg(&mut b, &t, &r, 128).unwrap();
    assert_eq!(a, b);
}

#[test]
fn saved_grid_cannot_overwrite_input_even_with_force() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.tif");
    let output = dir.path().join("result.svg");
    let region = Region {
        latitude: 0.,
        longitude: 0.,
        radius_m: 1000.,
    };
    let grid = Grid::analytic(9, |x, _| x).unwrap();
    coord2svg::raster::save_grid(&input, &region, &grid, "EGM2008").unwrap();
    let original = fs::read(&input).unwrap();
    let result = cmd(&[
        "--lat",
        "0",
        "--lon",
        "0",
        "--radius",
        "1km",
        "--input",
        input.to_str().unwrap(),
        "--input-datum",
        "EGM2008",
        "--save-grid",
        input.to_str().unwrap(),
        "--force",
        "--out",
        output.to_str().unwrap(),
    ]);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("cannot replace an input"));
    assert_eq!(fs::read(input).unwrap(), original);
    assert!(!output.exists());
}
