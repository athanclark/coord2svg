use coord2svg::{
    geometry::{self, Polygon, Vertex},
    terrain::{self, Grid, Index, Settings},
    union::{self, Shape},
};
use proptest::prelude::*;
fn rect(x: f64, y: f64, w: f64, h: f64) -> Polygon {
    vec![
        Vertex::new(x, y, 0.),
        Vertex::new(x + w, y, 0.),
        Vertex::new(x + w, y + h, 0.),
        Vertex::new(x, y + h, 0.),
    ]
}
fn area(shapes: &[Shape]) -> f64 {
    shapes.iter().map(|s| s.area()).sum()
}
fn contains(shapes: &[Shape], x: f64, y: f64) -> bool {
    shapes.iter().any(|s| s.contains(x, y))
}
#[test]
fn adjacency_t_junctions_and_overlaps_remove_internal_boundaries() {
    let pieces = vec![
        rect(0., 0., 0.5, 1.),
        rect(0.5, 0., 0.5, 0.5),
        rect(0.5, 0.5, 0.5, 0.5),
    ];
    let u = union::union(&pieces, 42.).unwrap();
    assert_eq!(u.len(), 1);
    assert_eq!(u[0].rings.len(), 1);
    assert_eq!(u[0].rings[0].len(), 4);
    assert_eq!(area(&u), 1.);
    let overlap = union::union(
        &[
            rect(0., 0., 0.75, 1.),
            rect(0.25, 0., 0.75, 1.),
            rect(0., 0., 1., 1.),
        ],
        0.,
    )
    .unwrap();
    assert_eq!(area(&overlap), 1.);
    assert!(contains(&overlap, 0.5, 0.5));
    assert_eq!(overlap[0].rings[0].len(), 4);
}
#[test]
fn holes_concavity_and_disconnected_islands_are_preserved() {
    let pieces = vec![
        rect(0., 0., 1., 0.25),
        rect(0., 0.75, 1., 0.25),
        rect(0., 0.25, 0.25, 0.5),
        rect(0.75, 0.25, 0.25, 0.5),
        rect(-1., 0., 0.25, 0.25),
    ];
    let u = union::union(&pieces, 0.).unwrap();
    assert_eq!(u.len(), 2);
    assert_eq!(u.iter().map(|s| s.rings.len()).sum::<usize>(), 3);
    assert!(!contains(&u, 0.5, 0.5));
    assert!(contains(&u, 0.1, 0.5));
    assert!(contains(&u, -0.9, 0.1));
    assert!((area(&u) - 0.8125).abs() < 1e-12);
    let l = union::union(&[rect(0., 0., 1., 0.25), rect(0., 0.25, 0.25, 0.75)], 0.).unwrap();
    assert!(!geometry::is_convex(&l[0].rings[0]));
    assert!(!contains(&l, 0.5, 0.5));
}
#[test]
fn unions_are_deterministic_empty_safe_and_precision_preserving() {
    assert!(union::union(&[], 0.).unwrap().is_empty());
    let mut pieces = vec![
        rect(0., 0., 1., 0.5),
        rect(0., 0.5, 1., 0.5),
        rect(-1., 0., 0.5, 0.5),
    ];
    let a = union::union(&pieces, 0.).unwrap();
    pieces.reverse();
    assert_eq!(a, union::union(&pieces, 0.).unwrap());
    let width = 1. / geometry::SNAP;
    let u = union::union(&[rect(0., 0., width, 1.)], 0.).unwrap();
    assert!((area(&u) - width).abs() < 1e-20);
    assert!(union::union(&[vec![Vertex::new(f64::NAN, 0., 0.); 3]], 0.).is_err());
}
#[test]
fn exported_union_coverage_matches_convex_working_pieces() {
    let grid = Grid::analytic(33, |x, y| 1000. * (x * x + y * y)).unwrap();
    let s = Settings {
        level_count: 9,
        ..Default::default()
    };
    let t = terrain::build(&grid, &s).unwrap();
    assert!(t.accuracy.union_area_change_m2 < 0.01);
    let idx = Index::new(&t);
    for x in -90..=90 {
        for y in -90..=90 {
            let (x, y) = (x as f64 / 100., y as f64 / 100.);
            if x * x + y * y > 0.99 {
                continue;
            }
            let expected = t
                .layers
                .iter()
                .rev()
                .find(|l| l.polygons.iter().any(|p| geometry::contains(p, x, y)))
                .map(|l| l.level.elevation_m);
            assert_eq!(idx.elevation(x, y), expected, "at {x},{y}");
        }
    }
    assert_eq!(idx.elevation(0., 0.), Some(0.));
}
proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn overlapping_rectangles_union_matches_membership(data in prop::collection::vec((-90i32..50,-90i32..50,1i32..40,1i32..40),1..12)) {
        let p:Vec<_>=data.iter().map(|&(x,y,w,h)|rect(x as f64/100.,y as f64/100.,w as f64/100.,h as f64/100.)).collect();
        let u=union::union(&p,0.).unwrap();
        prop_assert!(area(&u)<=p.iter().map(|p|geometry::area(p)).sum::<f64>()+1e-9);
        for x in -9..=9 {for y in -9..=9 {
            let (x,y)=(x as f64/10.+0.003,y as f64/10.+0.007);
            prop_assert_eq!(contains(&u,x,y),p.iter().any(|p|geometry::contains(p,x,y)));
        }}
    }
}

#[test]
fn cumulative_union_preserves_overlapping_boundaries() {
    let a = union::union(&[rect(0., 0., 1., 1.)], 0.).unwrap();
    let b = union::union(&[rect(0., 1., 1., 1.)], 0.).unwrap();
    let input: Vec<_> = a.into_iter().chain(b).collect();
    let u = union::union_shapes(&input, 0.).unwrap();
    assert_eq!(area(&u), 2.);
}
