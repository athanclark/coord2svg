use coord2svg::{
    output::{self, Report},
    sources::Region,
    style::{ElevationColor, ElevationWidth, EveryNColor, EveryNWidth, Style},
    terrain::{self, Grid, Settings},
};
use std::process::Command;
fn color(start: f64, every: usize, color: &str) -> EveryNColor {
    EveryNColor {
        starting_elevation_m: start,
        every,
        color: color.into(),
    }
}
fn width(start: f64, every: usize, width_px: f64) -> EveryNWidth {
    EveryNWidth {
        starting_elevation_m: start,
        every,
        width_px,
    }
}
fn settings() -> Settings {
    Settings {
        level_count: 11,
        elevation_filter: terrain::ElevationFilter {
            minimum_m: Some(0.),
            maximum_m: Some(100.),
        },
        ..Default::default()
    }
}
fn build(s: &Settings) -> terrain::Terrain {
    terrain::build(&Grid::analytic(17, |x, _| 200. * x).unwrap(), s).unwrap()
}
#[test]
fn every_n_uses_existing_level_steps_without_changing_geometry() {
    let mut s = settings();
    let baseline = build(&s);
    s.style.every_n_fill.push(color(0., 5, "#ff0000"));
    s.style.every_n_stroke_fill.push(color(0., 5, "#00ff00"));
    s.style.every_n_stroke_width.push(width(0., 5, 10.));
    let t = build(&s);
    assert_eq!(t.layers.len(), 11);
    assert_eq!(t.accuracy.polygon_count, baseline.accuracy.polygon_count);
    assert_eq!(t.accuracy.audit_rmse_m, baseline.accuracy.audit_rmse_m);
    for (i, l) in t.layers.iter().enumerate() {
        assert_eq!(l.level.base_level_index, Some(i));
        assert_eq!(l.level.fill == "#ff0000", i % 5 == 0);
        assert_eq!(l.level.stroke_fill == "#00ff00", i % 5 == 0);
        assert_eq!(l.level.stroke_width_px, if i % 5 == 0 { 10. } else { 0. });
        assert_eq!(l.polygons, baseline.layers[i].polygons);
    }
}
#[test]
fn anchors_between_levels_below_min_and_above_max() {
    let mut s = settings();
    s.style.every_n_fill.push(color(11., 3, "#ff0000"));
    let t = build(&s);
    let marked: Vec<_> = t
        .layers
        .iter()
        .filter(|l| l.level.fill == "#ff0000")
        .map(|l| l.level.elevation_m)
        .collect();
    assert_eq!(marked, vec![20., 50., 80.]);
    s.style.every_n_fill = vec![color(-1000., 5, "#ff0000")];
    assert_eq!(build(&s).layers[0].level.fill, "#ff0000");
    s.style.every_n_fill = vec![color(101., 1, "#ff0000")];
    assert!(build(&s).layers.iter().all(|l| l.level.fill != "#ff0000"));
}
#[test]
fn inserted_specific_levels_do_not_shift_patterns_and_specifics_win() {
    let mut s = settings();
    s.style.every_n_fill.push(color(0., 5, "#ff0000"));
    s.style.every_n_fill.push(color(0., 10, "#00ff00"));
    s.style.every_n_stroke_width.push(width(0., 5, 5.));
    s.style.specific_fill.push(ElevationColor {
        elevation_m: 15.,
        color: "#0000ff".into(),
    });
    s.style.specific_fill.push(ElevationColor {
        elevation_m: 50.,
        color: "#123456".into(),
    });
    s.style.specific_stroke_width.push(ElevationWidth {
        elevation_m: 50.,
        width_px: 2.,
    });
    let t = build(&s);
    assert_eq!(t.layers.len(), 12);
    let at = |z| {
        &t.layers
            .iter()
            .find(|l| l.level.elevation_m == z)
            .unwrap()
            .level
    };
    assert_eq!(at(0.).fill, "#00ff00");
    assert_eq!(at(100.).fill, "#00ff00");
    assert_eq!(at(50.).fill, "#123456");
    assert_eq!(at(50.).stroke_width_px, 2.);
    assert_eq!(at(15.).fill, "#0000ff");
    assert_eq!(at(15.).base_level_index, None);
    assert_eq!(at(15.).stroke_width_px, 0.);
    assert_eq!(at(50.).base_level_index, Some(5));
}
#[test]
fn interval_and_flat_terrain_patterns() {
    let mut s = settings();
    s.interval_m = Some(30.);
    s.style.every_n_fill.push(color(0., 2, "#ff0000"));
    let t = build(&s);
    assert_eq!(
        t.layers
            .iter()
            .map(|l| l.level.elevation_m)
            .collect::<Vec<_>>(),
        vec![0., 30., 60., 90., 100.]
    );
    assert_eq!(t.layers[2].level.fill, "#ff0000");
    assert_eq!(t.layers[4].level.fill, "#ff0000");
    let t = terrain::build(&Grid::analytic(9, |_, _| 42.).unwrap(), &s).unwrap();
    assert_eq!(t.layers[0].level.fill, "#ff0000");
}
#[test]
fn disabled_paint_overrides_all_styles_and_exports_none() {
    let mut s = settings();
    s.style.no_fill = true;
    s.style.no_stroke = true;
    s.style.stroke_width_px = 1.;
    s.style.every_n_fill.push(color(0., 1, "#ff0000"));
    s.style.every_n_stroke_width.push(width(0., 1, 10.));
    s.style.specific_fill.push(ElevationColor {
        elevation_m: 50.,
        color: "#00ff00".into(),
    });
    s.style.specific_stroke_width.push(ElevationWidth {
        elevation_m: 50.,
        width_px: 20.,
    });
    let t = build(&s);
    assert!(t.layers.iter().all(|l| l.level.fill == "none"
        && l.level.stroke_fill == "none"
        && l.level.stroke_width_px == 0.));
    assert_eq!(t.accuracy.audit_missing, 0);
    let r = Report::new(
        Region {
            latitude: 0.,
            longitude: 0.,
            radius_m: 1000.,
        },
        "synthetic".into(),
        vec![],
        &t,
    );
    let mut b = vec![];
    output::write_svg(&mut b, &t, &r, 1024).unwrap();
    let svg = String::from_utf8(b).unwrap();
    let doc = roxmltree::Document::parse(&svg).unwrap();
    for g in doc
        .descendants()
        .filter(|n| n.attribute("data-elevation-m").is_some())
    {
        assert_eq!(g.attribute("fill"), Some("none"));
        assert_eq!(g.attribute("stroke"), Some("none"));
        assert_eq!(g.attribute("stroke-width"), Some("0"));
    }
    s.style.no_stroke = false;
    assert!(
        build(&s)
            .layers
            .iter()
            .all(|l| l.level.fill == "none" && l.level.stroke_width_px > 0.)
    );
    s.style.no_fill = false;
    s.style.no_stroke = true;
    assert!(
        build(&s)
            .layers
            .iter()
            .all(|l| l.level.fill != "none" && l.level.stroke_width_px == 0.)
    );
}
#[test]
fn invalid_patterns_rejected_and_large_steps_safe() {
    for p in [
        color(0., 0, "#ff0000"),
        color(f64::NAN, 5, "#ff0000"),
        color(0., 5, "invalid"),
    ] {
        let s = Style {
            every_n_fill: vec![p],
            ..Default::default()
        };
        assert!(s.validate().is_err());
    }
    for p in [
        width(0., 0, 2.),
        width(f64::INFINITY, 1, 2.),
        width(0., 1, -1.),
    ] {
        let s = Style {
            every_n_stroke_width: vec![p],
            ..Default::default()
        };
        assert!(s.validate().is_err());
    }
    let mut s = settings();
    s.style.every_n_fill.push(color(0., usize::MAX, "#ff0000"));
    assert_eq!(build(&s).layers[0].level.fill, "#ff0000");
}
fn cli(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_coord2svg"))
        .args([
            "--lat",
            "0",
            "--lon",
            "0",
            "--radius",
            "1km",
            "--demo",
            "--dry-run",
        ])
        .args(args)
        .output()
        .unwrap()
}
#[test]
fn cli_parsers_pattern_defaults_and_rejections() {
    let out = cli(&[
        "--no-fill",
        "--no-stroke",
        "--every-n-fill",
        "-1mi,5,#123456",
        "--every-n-stroke-fill",
        "0ft,5,#abcdef",
        "--every-n-stroke-width",
        "0ft,5,10px",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let plan: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let s = &plan["style"];
    assert_eq!(s["no_fill"], true);
    assert_eq!(s["no_stroke"], true);
    assert_eq!(s["every_n_fill"][0]["starting_elevation_m"], -1609.344);
    let only_color = cli(&["--every-n-stroke-fill", "0ft,5,#ff0000"]);
    let plan: serde_json::Value = serde_json::from_slice(&only_color.stdout).unwrap();
    assert_eq!(plan["style"]["stroke_width_px"], 0.);
    assert_eq!(plan["style"]["every_n_stroke_width"][0]["width_px"], 1.);
    for bad in [
        "0ft,0,#123456",
        "0ft,-5,#123456",
        "0ft,1.5,#123456",
        "NaN,5,#123456",
        "0ft,5,#bad",
        "0ft,5,#123456,extra",
        "0ft,5",
    ] {
        assert!(!cli(&["--every-n-fill", bad]).status.success(), "{bad}");
    }
    assert!(
        !cli(&["--every-n-stroke-width", "0ft,5,-10px"])
            .status
            .success()
    );
}
