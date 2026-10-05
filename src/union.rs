//! True per-level Boolean unions on the same integer grid as convex pieces.
use crate::geometry::{self, Polygon, SNAP, Vertex};
use anyhow::{Result, ensure};
use i_overlay::{
    core::{fill_rule::FillRule, overlay::Overlay, overlay_rule::OverlayRule},
    i_float::int::point::IntPoint,
};

#[derive(Clone, Debug, PartialEq)]
pub struct Shape {
    pub rings: Vec<Polygon>,
}
impl Shape {
    pub fn area(&self) -> f64 {
        self.rings
            .iter()
            .map(|p| geometry::area(p))
            .sum::<f64>()
            .abs()
    }
    pub fn bounds(&self) -> (f64, f64, f64, f64) {
        self.rings.iter().map(|p| geometry::bounds(p)).fold(
            (
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
            ),
            |a, b| (a.0.min(b.0), a.1.max(b.1), a.2.min(b.2), a.3.max(b.3)),
        )
    }
    pub fn contains(&self, x: f64, y: f64) -> bool {
        if !x.is_finite() || !y.is_finite() {
            return false;
        }
        let q = Vertex::new(x, y, 0.);
        let mut inside = false;
        for ring in &self.rings {
            for i in 0..ring.len() {
                let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
                let (dx, dy) = (b.x - a.x, b.y - a.y);
                let len = dx.hypot(dy);
                let along = (x - a.x) * dx + (y - a.y) * dy;
                if len > 0.
                    && geometry::cross(a, b, q).abs() <= len / SNAP
                    && along >= -len / SNAP
                    && along <= len * len + len / SNAP
                {
                    return true;
                }
                if (a.y > y) != (b.y > y) && x < a.x + (y - a.y) * (b.x - a.x) / (b.y - a.y) {
                    inside = !inside;
                }
            }
        }
        inside
    }
}
pub fn union(polygons: &[Polygon], z: f64) -> Result<Vec<Shape>> {
    if polygons.is_empty() {
        return Ok(vec![]);
    }
    ensure!(
        polygons.iter().all(|p| geometry::is_convex(p)),
        "union input must contain valid convex pieces"
    );
    let contours: Vec<Vec<IntPoint<i64>>> = polygons
        .iter()
        .map(|p| {
            p.iter()
                .map(|v| {
                    let (x, y) = v.key();
                    IntPoint::new(x, y)
                })
                .collect()
        })
        .collect();
    // NonZero is essential: overlapping pieces add coverage rather than XOR.
    let shapes =
        Overlay::<i64>::from_subj(&contours).overlay(OverlayRule::Union, FillRule::NonZero);
    canonical_shapes(shapes, z)
}

/// Union already-normalized shapes, preserving the opposite winding of holes.
pub fn union_shapes(shapes: &[Shape], z: f64) -> Result<Vec<Shape>> {
    ensure!(z.is_finite(), "union elevation must be finite");
    ensure!(
        shapes.iter().flat_map(|s| &s.rings).all(|r| r.len() >= 3
            && r.iter()
                .all(|v| v.x.is_finite() && v.y.is_finite() && v.x.abs() <= 2. && v.y.abs() <= 2.)),
        "invalid union contour"
    );
    let contours: Vec<Vec<IntPoint<i64>>> = shapes
        .iter()
        .flat_map(|s| &s.rings)
        .map(|p| {
            p.iter()
                .map(|v| {
                    let (x, y) = v.key();
                    IntPoint::new(x, y)
                })
                .collect()
        })
        .collect();
    let result =
        Overlay::<i64>::from_subj(&contours).overlay(OverlayRule::Union, FillRule::NonZero);
    canonical_shapes(result, z)
}
fn canonical_shapes(shapes: Vec<Vec<Vec<IntPoint<i64>>>>, z: f64) -> Result<Vec<Shape>> {
    let mut result = vec![];
    for shape in shapes {
        ensure!(!shape.is_empty(), "union produced an empty shape");
        let mut rings: Vec<Polygon> = shape
            .into_iter()
            .map(|p| {
                p.into_iter()
                    .map(|v| Vertex::new(v.x as f64 / SNAP, v.y as f64 / SNAP, z))
                    .collect()
            })
            .collect();
        // Canonical start vertices and ordering make serialization reproducible.
        for ring in &mut rings {
            ensure!(ring.len() >= 3, "union produced a degenerate ring");
            let start = (0..ring.len()).min_by_key(|&i| ring[i].key()).unwrap();
            ring.rotate_left(start);
        }
        rings[1..].sort_by_key(|p| p[0].key());
        result.push(Shape { rings });
    }
    result.sort_by_key(|s| s.rings[0][0].key());
    Ok(result)
}
