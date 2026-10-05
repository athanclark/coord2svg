use coord2svg::{
    output::{self, Report},
    sources::Region,
    style::{self, ColorStop, ElevationColor, ElevationWidth, Gradient, GradientMode, Style},
    terrain::{self, Grid, Settings},
};
use std::process::Command;
fn gradient(mode: GradientMode) -> Gradient {
    Gradient {
        lowest: "#ff0000".into(),
        highest: "#0000ff".into(),
        mode,
        stops: vec![],
    }
}
#[test]
fn known_interpolation_values_and_exact_endpoints() {
    for mode in [
        GradientMode::Srgb,
        GradientMode::LinearRgb,
        GradientMode::HsvShort,
        GradientMode::HsvLong,
        GradientMode::Rainbow,
    ] {
        let g = gradient(mode);
        g.validate().unwrap();
        assert_eq!(g.at(0.), "#ff0000");
        assert_eq!(g.at(1.), "#0000ff");
        assert_eq!(g.at(-1.), "#ff0000");
        assert_eq!(g.at(2.), "#0000ff");
    }
    assert_eq!(gradient(GradientMode::Srgb).at(0.5), "#800080");
    assert_eq!(gradient(GradientMode::LinearRgb).at(0.5), "#bc00bc");
    assert_eq!(gradient(GradientMode::HsvShort).at(0.5), "#ff00ff");
    assert_eq!(gradient(GradientMode::HsvLong).at(0.5), "#00ff00");
    let g = Gradient {
        mode: GradientMode::LinearRgb,
        ..Default::default()
    };
    assert_eq!(g.at(0.5), "#bcbcbc");
}
#[test]
fn hsv_achromatic_and_hue_wrap() {
    for mode in [GradientMode::HsvShort, GradientMode::HsvLong] {
        let g = Gradient {
            lowest: "#ffffff".into(),
            highest: "#0000ff".into(),
            mode,
            stops: vec![],
        };
        assert_eq!(g.at(0.5), "#8080ff");
        let flat = Gradient {
            lowest: "#808080".into(),
            highest: "#808080".into(),
            mode,
            stops: vec![],
        };
        assert_eq!(flat.at(0.5), "#808080");
    }
    let g = Gradient {
        lowest: "#ff0080".into(),
        highest: "#ff8000".into(),
        mode: GradientMode::HsvShort,
        stops: vec![],
    };
    assert_eq!(g.at(0.5), "#ff0000");
}
#[test]
fn custom_stops_are_sorted_and_exact() {
    let g = Gradient {
        stops: vec![
            ColorStop {
                position: 0.75,
                color: "#0000ff".into(),
            },
            ColorStop {
                position: 0.25,
                color: "#ff0000".into(),
            },
        ],
        ..Default::default()
    };
    g.validate().unwrap();
    assert_eq!(g.at(0.25), "#ff0000");
    assert_eq!(g.at(0.75), "#0000ff");
    assert_eq!(g.at(0.5), "#800080");
    let g = Gradient {
        mode: GradientMode::Rainbow,
        ..Default::default()
    };
    for (i, c) in [
        "#ff0000", "#ffff00", "#00ff00", "#00ffff", "#0000ff", "#ff00ff",
    ]
    .iter()
    .enumerate()
    {
        assert_eq!(g.at((i + 1) as f64 / 7.), *c);
    }
}
#[test]
fn units_and_malformed_values() {
    assert_eq!(style::parse_elevation("1mi").unwrap(), 1609.344);
    assert_eq!(style::parse_elevation("-10000 ft").unwrap(), -3048.);
    assert_eq!(style::canonical_color(" #AaBBcC ").unwrap(), "#aabbcc");
    for s in [
        "red",
        "#fff",
        "#12345678",
        "#gg0000",
        "#ab🙂",
        "#123456\" onload=\"x",
    ] {
        assert!(style::parse_color(s).is_err());
    }
    for s in ["-1px", "inf", "NaN", "10m", "1e309px"] {
        assert!(style::parse_width(s).is_err());
    }
    assert_eq!(style::parse_width("10 px").unwrap(), 10.);
}
#[test]
fn invalid_stop_and_duplicate_overrides_are_rejected() {
    for position in [0., 1., f64::NAN, f64::INFINITY] {
        let g = Gradient {
            stops: vec![ColorStop {
                position,
                color: "#ff0000".into(),
            }],
            ..Default::default()
        };
        assert!(g.validate().is_err());
    }
    let mut s = Style {
        specific_fill: vec![
            ElevationColor {
                elevation_m: 0.,
                color: "#ff0000".into()
            };
            2
        ],
        ..Default::default()
    };
    assert!(s.validate().is_err());
    s.specific_fill.clear();
    s.fill.stops = vec![
        ColorStop {
            position: 0.5,
            color: "#ff0000".into()
        };
        2
    ];
    assert!(s.validate().is_err());
    s.fill.stops.truncate(1);
    s.fill.mode = GradientMode::Rainbow;
    assert!(s.validate().is_err());
}
#[test]
fn post_exclusion_endpoints_overrides_and_exported_strokes() {
    let mut s = Settings {
        elevation_filter: terrain::ElevationFilter {
            minimum_m: Some(0.),
            maximum_m: Some(200.),
        },
        level_count: 3,
        ..Default::default()
    };
    s.style.fill = Gradient {
        lowest: "#123456".into(),
        highest: "#abcdef".into(),
        ..Default::default()
    };
    s.style.stroke = gradient(GradientMode::LinearRgb);
    s.style.stroke_width_px = 2.;
    s.style.specific_fill.push(ElevationColor {
        elevation_m: 42.,
        color: "#00ff00".into(),
    });
    s.style.specific_stroke_fill.push(ElevationColor {
        elevation_m: 42.,
        color: "#00ffff".into(),
    });
    s.style.specific_stroke_width.push(ElevationWidth {
        elevation_m: 42.,
        width_px: 10.,
    });
    let t = terrain::build(&Grid::analytic(17, |x, _| 500. * x).unwrap(), &s).unwrap();
    assert_eq!(t.layers.first().unwrap().level.fill, "#123456");
    assert_eq!(t.layers.last().unwrap().level.fill, "#abcdef");
    let special = t
        .layers
        .iter()
        .find(|l| l.level.elevation_m == 42.)
        .unwrap();
    assert!(!special.polygons.is_empty());
    assert_eq!(special.level.fill, "#00ff00");
    assert_eq!(special.level.stroke_fill, "#00ffff");
    assert_eq!(special.level.stroke_width_px, 10.);
    assert!(t.accuracy.audit_max_abs_error_m.unwrap() <= t.accuracy.quantization_bound_m + 1e-6);
    let report = Report::new(
        Region {
            latitude: 0.,
            longitude: 0.,
            radius_m: 1000.,
        },
        "synthetic".into(),
        vec![],
        &t,
    );
    let mut bytes = vec![];
    output::write_svg(&mut bytes, &t, &report, 1024).unwrap();
    let svg = String::from_utf8(bytes).unwrap();
    let doc = roxmltree::Document::parse(&svg).unwrap();
    let group = doc
        .descendants()
        .find(|n| n.attribute("data-elevation-m") == Some("42"))
        .unwrap();
    assert_eq!(group.attribute("fill"), Some("#00ff00"));
    assert_eq!(group.attribute("stroke"), Some("#00ffff"));
    assert_eq!(group.attribute("stroke-width"), Some("10"));
    assert!(
        group
            .children()
            .filter(|n| n.has_tag_name("path"))
            .all(|n| n.attribute("vector-effect") == Some("non-scaling-stroke"))
    );
    assert_eq!(report.schema_version, 3);
}
#[test]
fn excluded_specific_elevations_are_not_resurrected_and_flat_uses_lowest() {
    let mut s = Settings::default();
    s.style.fill = gradient(GradientMode::Rainbow);
    s.style.specific_fill.push(ElevationColor {
        elevation_m: 0.,
        color: "#00ff00".into(),
    });
    let t = terrain::build(&Grid::analytic(9, |_, _| 42.).unwrap(), &s).unwrap();
    assert_eq!(t.layers.len(), 1);
    assert_eq!(t.layers[0].level.fill, "#ff0000");
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
    assert!(r.warnings.iter().any(|s| s.contains("ignored")));
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
fn cli_parses_every_style_flag_and_conflicts() {
    let result = cli(&[
        "--lowest-fill",
        "#aabbcc",
        "--highest-fill",
        "#123456",
        "--fill-gradient",
        "linear-rgb",
        "--fill-stop",
        "0.5,#ff0000",
        "--lowest-stroke-fill",
        "#00ff00",
        "--highest-stroke-fill",
        "#0000ff",
        "--stroke-gradient",
        "hsv-long",
        "--stroke-stop",
        "0.25,#ffffff",
        "--stroke-width",
        "2px",
        "--specific-fill",
        "0ft,#ff0000",
        "--specific-fill",
        "1mi,#ffff00",
        "--specific-stroke-fill",
        "-1km,#ff00ff",
        "--specific-stroke-width",
        "0ft,10px",
    ]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let plan: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(plan["style"]["specific_fill"][1]["elevation_m"], 1609.344);
    assert_eq!(plan["style"]["specific_stroke_width"][0]["width_px"], 10.);
    let only_specific = cli(&["--specific-stroke-fill", "0ft,#ff0000"]);
    assert!(only_specific.status.success());
    let plan: serde_json::Value = serde_json::from_slice(&only_specific.stdout).unwrap();
    assert_eq!(plan["style"]["stroke_width_px"], 0.);
    assert_eq!(plan["style"]["specific_stroke_width"][0]["width_px"], 1.);
    let solid = cli(&["--stroke-fill", "#ff0000"]);
    assert!(
        solid.status.success(),
        "{}",
        String::from_utf8_lossy(&solid.stderr)
    );
    let plan: serde_json::Value = serde_json::from_slice(&solid.stdout).unwrap();
    assert_eq!(plan["style"]["stroke_width_px"], 1.);
    assert!(
        !cli(&[
            "--stroke-fill",
            "#ff0000",
            "--lowest-stroke-fill",
            "#000000"
        ])
        .status
        .success()
    );
    assert!(
        !cli(&["--fill-stop", "0.5,#ff0000", "--fill-gradient", "rainbow"])
            .status
            .success()
    );
    assert!(
        !cli(&[
            "--specific-fill",
            "0ft,#ff0000",
            "--specific-fill",
            "0m,#00ff00"
        ])
        .status
        .success()
    );
    assert!(!cli(&["--stroke-width", "-1px"]).status.success());
}
