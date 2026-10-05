//! Portable raster regressions: no external viewer or image utility is needed.
use coord2svg::{
    output::{self, Report},
    sources::Region,
    terrain::{self, Grid, Settings},
};
use resvg::{
    tiny_skia::{Pixmap, Transform},
    usvg,
};
fn render(grid: Grid, s: Settings) -> Pixmap {
    let t = terrain::build(&grid, &s).unwrap();
    let r = Report::new(
        Region {
            latitude: 0.,
            longitude: 0.,
            radius_m: s.radius_m,
        },
        "synthetic".into(),
        vec![],
        &t,
    );
    let mut bytes = vec![];
    output::write_svg(&mut bytes, &t, &r, 512).unwrap();
    let tree = usvg::Tree::from_str(
        &String::from_utf8(bytes).unwrap(),
        &usvg::Options::default(),
    )
    .unwrap();
    let mut image = Pixmap::new(512, 512).unwrap();
    resvg::render(&tree, Transform::default(), &mut image.as_mut());
    image
}
#[test]
fn no_stroke_has_no_mesh_or_band_transparency_seams() {
    let mut s = Settings {
        level_count: 64,
        ..Default::default()
    };
    s.style.no_stroke = true;
    s.style.fill.lowest = "#888888".into();
    s.style.fill.highest = "#888888".into();
    // Many adjacent bands and irregular convex pieces, all painted the same
    // reference color: every interior pixel must have that color and opacity.
    let image = render(Grid::analytic(41, |x, y| 500. * x - 200. * y).unwrap(), s);
    for y in 0..512 {
        for x in 0..512 {
            let nx = (x as f64 + 0.5) / 256. - 1.;
            let ny = 1. - (y as f64 + 0.5) / 256.;
            if nx * nx + ny * ny < 0.8 * 0.8 {
                let p = image.pixel(x, y).unwrap();
                assert_eq!(
                    (p.red(), p.green(), p.blue(), p.alpha()),
                    (136, 136, 136, 255),
                    "seam at pixel {x},{y}"
                );
            }
        }
    }
}
#[test]
fn unioned_footprints_preserve_excluded_holes_and_transparency() {
    let mut s = Settings {
        level_count: 16,
        elevation_filter: terrain::ElevationFilter {
            minimum_m: Some(100.),
            maximum_m: Some(500.),
        },
        ..Default::default()
    };
    s.style.no_stroke = true;
    let image = render(
        Grid::analytic(33, |x, y| 1000. * (x * x + y * y)).unwrap(),
        s,
    );
    assert_eq!(
        image.pixel(256, 256).unwrap().alpha(),
        0,
        "depression hole was filled"
    );
    assert_eq!(
        image.pixel(384, 256).unwrap().alpha(),
        255,
        "selected annulus is not opaque"
    );
    assert_eq!(
        image.pixel(490, 256).unwrap().alpha(),
        0,
        "excluded high terrain was restored"
    );
}
