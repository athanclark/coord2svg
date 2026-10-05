use coord2svg::{
    geometry::{self, Boundary, Vertex},
    terrain::{self, Grid, Index, Settings},
};
use proptest::prelude::*;

fn settings() -> Settings {
    Settings {
        level_count: 17,
        boundary_tolerance_m: 0.2,
        audit_samples: 1024,
        ..Default::default()
    }
}
fn verify(f: impl Fn(f64, f64) -> f64) -> terrain::Terrain {
    let grid = Grid::analytic(41, f).unwrap();
    let s = settings();
    let t = terrain::build(&grid, &s).unwrap();
    assert!(
        t.layers
            .iter()
            .flat_map(|l| &l.polygons)
            .all(|p| geometry::is_convex(p))
    );
    assert!(
        t.layers
            .windows(2)
            .all(|w| w[0].level.elevation_m < w[1].level.elevation_m
                && w[0].level.gray >= w[1].level.gray)
    );
    assert!(t.accuracy.audit_max_abs_error_m.unwrap() <= t.accuracy.quantization_bound_m + 1e-5);
    assert!(t.accuracy.coverage_fraction > 0.999);
    assert_eq!(t.accuracy.audit_missing, 0);
    assert!(t.accuracy.decomposition_area_change_m2 < 0.01);
    assert_eq!(t.layers.first().unwrap().level.fill, "#ffffff");
    if t.layers.len() > 1 {
        assert_eq!(t.layers.last().unwrap().level.fill, "#000000");
    }
    assert!(
        t.layers
            .iter()
            .flat_map(|l| l.polygons.iter().flatten())
            .all(|v| v.x.hypot(v.y) <= 1. + 1e-9)
    );
    t
}
#[test]
fn plane() {
    verify(|x, y| 500. * x - 200. * y);
}
#[test]
fn isolated_hill() {
    verify(|x, y| 3000. * (-8. * (x * x + y * y)).exp());
}
#[test]
fn saddle() {
    verify(|x, y| 500. * x * y);
}
#[test]
fn all_below_sea() {
    let t = verify(|x, y| -7000. + 500. * x + 250. * y);
    assert!(t.accuracy.maximum_elevation_m < 0.);
}
#[test]
fn mixed_seabed_land() {
    let t = verify(|x, y| -5000. + 9000. * (-10. * (x * x + y * y)).exp());
    assert!(t.accuracy.minimum_elevation_m < 0. && t.accuracy.maximum_elevation_m > 0.);
}
#[test]
fn flat_terrain() {
    let t = verify(|_, _| 42.);
    assert_eq!(t.layers.len(), 1);
    assert!(t.accuracy.audit_max_abs_error_m.unwrap() < 1e-12);
    assert!(t.accuracy.polygon_count < t.accuracy.raw_piece_count / 2);
}
#[test]
fn depression_preserves_hole() {
    let t = verify(|x, y| 1000. * (x * x + y * y));
    let idx = Index::new(&t);
    assert_eq!(idx.elevation(0., 0.), Some(0.));
    assert!(idx.elevation(0.8, 0.).unwrap() > 600.);
    // No higher polygon covers the depression center.
    assert!(
        t.layers
            .iter()
            .skip(1)
            .all(|l| l.polygons.iter().all(|p| !geometry::contains(p, 0., 0.)))
    );
}
#[test]
fn separated_islands_preserve_channel() {
    let t = verify(|x, y| {
        -100.
            + 1000. * (-80. * ((x - 0.45).powi(2) + y * y)).exp()
            + 1000. * (-80. * ((x + 0.45).powi(2) + y * y)).exp()
    });
    let idx = Index::new(&t);
    assert!(idx.elevation(0., 0.).unwrap() < 0.);
    assert!(idx.elevation(0.45, 0.).unwrap() > 500.);
}
#[test]
fn missing_data_is_transparent() {
    let grid = Grid::analytic(41, |x, y| {
        if x.abs() < 0.2 && y.abs() < 0.2 {
            f64::NAN
        } else {
            100. * x
        }
    })
    .unwrap();
    let t = terrain::build(&grid, &settings()).unwrap();
    assert!(t.accuracy.coverage_fraction < 0.98);
    let idx = Index::new(&t);
    assert!(idx.elevation(0., 0.).is_none());
    assert!(idx.elevation(0.8, 0.).is_some());
}
#[test]
fn no_valid_data_is_error() {
    assert!(
        terrain::build(&Grid::analytic(3, |_, _| f64::NAN).unwrap(), &settings())
            .unwrap_err()
            .to_string()
            .contains("no valid terrain")
    );
}
#[test]
fn outside_peaks_do_not_set_color_range() {
    let grid = Grid::analytic(5, |x, y| {
        if x.abs() == 1. && y.abs() == 1. {
            1e6
        } else {
            x
        }
    })
    .unwrap();
    let t = terrain::build(&grid, &settings()).unwrap();
    assert!(t.accuracy.maximum_elevation_m < 1e6);
}
#[test]
fn large_radius_changes_only_physical_metrics() {
    let grid = Grid::analytic(21, |x, y| 500. * x + 200. * y).unwrap();
    let a = terrain::build(
        &grid,
        &Settings {
            boundary_tolerance_m: 0.01,
            ..settings()
        },
    )
    .unwrap();
    let b = terrain::build(
        &grid,
        &Settings {
            radius_m: 1e6,
            boundary_tolerance_m: 10.,
            ..settings()
        },
    )
    .unwrap();
    assert_eq!(a.accuracy.polygon_count, b.accuracy.polygon_count);
    assert!((a.accuracy.coverage_fraction - b.accuracy.coverage_fraction).abs() < 1e-12);
    assert!((b.accuracy.covered_area_m2 / a.accuracy.covered_area_m2 - 1e6).abs() < 1e-7);
}
#[test]
fn boundary_error_is_bounded() {
    for radius in [1000., 10_000., 100_000., 1_000_000.] {
        let b = Boundary::new(radius, 0.01 * radius).unwrap();
        assert!(b.radial_error_m <= 0.01 * radius);
        for w in b.points.windows(2) {
            let midpoint = Vertex::new((w[0].x + w[1].x) / 2., (w[0].y + w[1].y) / 2., 0.);
            assert!((radius * (1. - midpoint.x.hypot(midpoint.y)) - b.radial_error_m).abs() < 1e-8);
        }
    }
}
#[test]
fn invalid_settings_fail() {
    let grid = Grid::analytic(3, |x, _| x).unwrap();
    for interval in [0., -1., f64::NAN, f64::INFINITY, 1e-8] {
        assert!(
            terrain::build(
                &grid,
                &Settings {
                    interval_m: Some(interval),
                    ..settings()
                }
            )
            .is_err()
        );
    }
    assert!(Boundary::new(1000., f64::NAN).is_err());
    assert!(Boundary::new(1000., 1e-30).is_err());
    assert!(
        terrain::build(
            &grid,
            &Settings {
                max_polygons: 1,
                ..settings()
            }
        )
        .is_err()
    );
    assert!(Grid::new(4, vec![0.; 3]).is_err());
}
#[test]
fn interval_endpoints_and_midpoint_ties() {
    let h = terrain::elevations(
        -130.,
        90.,
        &Settings {
            interval_m: Some(100.),
            ..settings()
        },
    )
    .unwrap();
    assert_eq!(h, vec![-130., -30., 70., 90.]);
    assert_eq!(terrain::classify(-80., &[-80., 20., 80.]), 1);
}
#[test]
fn color_scale_and_clamping() {
    assert_eq!(terrain::grayscale(-10., 0., 100.), 255);
    assert_eq!(terrain::grayscale(110., 0., 100.), 0);
    assert_eq!(terrain::grayscale(50., 0., 100.), 128);
    assert_eq!(terrain::grayscale(42., 42., 42.), 255);
    let g = Grid::analytic(13, |x, _| 100. * x).unwrap();
    let t = terrain::build(
        &g,
        &Settings {
            color_range: Some((-200., 200.)),
            ..settings()
        },
    )
    .unwrap();
    assert!(t.layers[0].level.gray < 255);
    assert!(t.layers.last().unwrap().level.gray > 0);
}
#[test]
fn merged_and_unmerged_agree() {
    let grid = Grid::analytic(21, |x, y| 300. * x * x - 200. * y * y).unwrap();
    let a = terrain::build(&grid, &settings()).unwrap();
    let b = terrain::build(
        &grid,
        &Settings {
            merge: false,
            ..settings()
        },
    )
    .unwrap();
    let ia = Index::new(&a);
    let ib = Index::new(&b);
    for y in -9..=9 {
        for x in -9..=9 {
            let (x, y) = (x as f64 / 10., y as f64 / 10.);
            if x * x + y * y < 0.95 {
                assert_eq!(ia.elevation(x, y), ib.elevation(x, y));
            }
        }
    }
}
#[test]
fn deterministic_serialized_geometry() {
    let grid = Grid::analytic(13, |x, y| 100. * x * y).unwrap();
    let a = terrain::build(&grid, &settings()).unwrap();
    let b = terrain::build(&grid, &settings()).unwrap();
    assert_eq!(format!("{:?}", a), format!("{:?}", b));
}
#[test]
fn convexity_rejects_stars_and_repeated_loops() {
    let square = [(-1., -1.), (1., -1.), (1., 1.), (-1., 1.)];
    let doubled: Vec<_> = square
        .iter()
        .chain(square.iter())
        .map(|&(x, y)| Vertex::new(x, y, 0.))
        .collect();
    assert!(!geometry::is_convex(&doubled));
    let star: Vec<_> = (0..5)
        .map(|i| {
            let angle = std::f64::consts::TAU * 2. * i as f64 / 5.;
            Vertex::new(angle.cos(), angle.sin(), 0.)
        })
        .collect();
    assert!(!geometry::is_convex(&star));
}
#[test]
fn independent_convexity_supporting_lines_and_shared_edge_coverage() {
    let g = Grid::analytic(17, |x, y| 100. * x + 20. * y).unwrap();
    let t = terrain::build(
        &g,
        &Settings {
            level_count: 9,
            ..settings()
        },
    )
    .unwrap();
    let idx = Index::new(&t);
    for x in -80..=80 {
        let x = x as f64 / 100.;
        assert!(idx.elevation(x, 0.).is_some(), "shared edge gap at {x}");
    }
    for p in t.layers.iter().flat_map(|l| &l.polygons) {
        let mut turn = 0.;
        for i in 0..p.len() {
            let a = p[i];
            let b = p[(i + 1) % p.len()];
            let c = p[(i + 2) % p.len()];
            let angle = ((b.x - a.x) * (c.y - b.y) - (b.y - a.y) * (c.x - b.x))
                .atan2((b.x - a.x) * (c.x - b.x) + (b.y - a.y) * (c.y - b.y));
            turn += angle;
        }
        assert!(
            (turn - std::f64::consts::TAU).abs() < 1e-6,
            "polygon winds more than once"
        );
    }
}
proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test] fn random_planes_preserve_convexity_and_error(a in -10000f64..10000.,b in -10000f64..10000.,c in -10000f64..10000.) {
        let grid=Grid::analytic(9,|x,y|a*x+b*y+c).unwrap();let t=terrain::build(&grid,&Settings{audit_samples:128,..settings()}).unwrap();
        prop_assert!(t.layers.iter().flat_map(|l|&l.polygons).all(|p|geometry::is_convex(p)));
        prop_assert!(t.accuracy.audit_max_abs_error_m.unwrap()<=t.accuracy.quantization_bound_m+1e-4);
        prop_assert!(t.accuracy.coverage_fraction>0.999);
    }
    #[test] fn clipping_never_increases_area(threshold in -1f64..1.) {
        let p=vec![Vertex::new(-1.,-1.,-1.),Vertex::new(1.,-1.,1.),Vertex::new(1.,1.,1.),Vertex::new(-1.,1.,-1.)];
        let q=geometry::clip(&p,|v|v.z-threshold);
        prop_assert!(geometry::is_convex(&q));prop_assert!(geometry::area(&q)<=geometry::area(&p)+1e-12);
        prop_assert!((geometry::area(&q)-2.*(1.-threshold)).abs()<1e-10);
    }
}

#[test]
fn exact_convexity_rejects_subpixel_concavity() {
    let p = vec![
        Vertex::new(-1., -1., 0.),
        Vertex::new(1., -1., 0.),
        Vertex::new(1., 1., 0.),
        Vertex::new(0., 1. - 1e-10, 0.),
        Vertex::new(-1., 1., 0.),
    ];
    assert!(!geometry::is_convex(&p));
}
