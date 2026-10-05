//! Convex clipping in radius-normalized coordinates. Heights remain in metres.
use anyhow::{Result, ensure};
use std::collections::{HashMap, VecDeque};

pub const SNAP: f64 = 1e10;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vertex {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}
impl Vertex {
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }
    fn between(a: Self, b: Self, t: f64) -> Self {
        Self::new(
            a.x + (b.x - a.x) * t,
            a.y + (b.y - a.y) * t,
            a.z + (b.z - a.z) * t,
        )
    }
    pub fn key(self) -> (i64, i64) {
        (
            (self.x * SNAP).round() as i64,
            (self.y * SNAP).round() as i64,
        )
    }
}
pub type Polygon = Vec<Vertex>;

pub fn cross(a: Vertex, b: Vertex, c: Vertex) -> f64 {
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
}
pub fn area(p: &[Vertex]) -> f64 {
    if p.len() < 3 {
        return 0.;
    }
    // Translate to the first vertex to avoid cancellation in tiny polygons.
    (1..p.len() - 1)
        .map(|i| cross(p[0], p[i], p[i + 1]))
        .sum::<f64>()
        / 2.
}
pub fn is_convex(p: &[Vertex]) -> bool {
    if p.len() < 3
        || area(p) <= 0.
        || !p
            .iter()
            .all(|v| v.x.is_finite() && v.y.is_finite() && v.x.abs() <= 2. && v.y.abs() <= 2.)
    {
        return false;
    }
    let points: Vec<_> = p.iter().map(|v| v.key()).collect();
    if !(0..p.len()).all(|i| {
        key_cross(
            points[i],
            points[(i + 1) % p.len()],
            points[(i + 2) % p.len()],
        ) >= 0
    }) {
        return false;
    }
    // Local turn signs alone accept self-touching loops and star polygons.
    // A convex ring has distinct vertices and every edge is a supporting line.
    let mut keys = std::collections::HashSet::with_capacity(p.len());
    if !points.iter().all(|&v| keys.insert(v)) {
        return false;
    }
    (0..p.len()).all(|i| {
        points
            .iter()
            .all(|&v| key_cross(points[i], points[(i + 1) % p.len()], v) >= 0)
    })
}
fn key_cross(a: (i64, i64), b: (i64, i64), c: (i64, i64)) -> i128 {
    // Normalized coordinates are bounded by +/-2: products fit safely in i128.
    (b.0 as i128 - a.0 as i128) * (c.1 as i128 - a.1 as i128)
        - (b.1 as i128 - a.1 as i128) * (c.0 as i128 - a.0 as i128)
}
pub fn contains(p: &[Vertex], x: f64, y: f64) -> bool {
    if p.len() < 3 || !x.is_finite() || !y.is_finite() {
        return false;
    }
    let q = Vertex::new(x, y, 0.);
    (0..p.len()).all(|i| {
        let a = p[i];
        let b = p[(i + 1) % p.len()];
        cross(a, b, q) >= -(b.x - a.x).hypot(b.y - a.y) / SNAP
    })
}

/// Sutherland-Hodgman clipping, keeping f(vertex) >= 0.
pub fn clip(p: &[Vertex], f: impl Fn(Vertex) -> f64) -> Polygon {
    let mut out = Vec::with_capacity(p.len() + 1);
    if p.is_empty() {
        return out;
    }
    let mut a = *p.last().unwrap();
    let mut fa = f(a);
    for &b in p {
        let fb = f(b);
        if (fa >= 0.) != (fb >= 0.) {
            // Canonical endpoint ordering makes shared intersections reproducible.
            let (v, w, fv, fw) = if (a.x, a.y) < (b.x, b.y) {
                (a, b, fa, fb)
            } else {
                (b, a, fb, fa)
            };
            out.push(Vertex::between(v, w, (fv / (fv - fw)).clamp(0., 1.)));
        }
        if fb >= 0. {
            out.push(b);
        }
        a = b;
        fa = fb;
    }
    out.dedup_by(|a, b| a.x == b.x && a.y == b.y);
    if out.len() > 1 && out.first() == out.last() {
        out.pop();
    }
    out
}

#[derive(Clone, Debug)]
pub struct Boundary {
    pub points: Polygon,
    pub radial_error_m: f64,
    inner_squared: f64,
}
impl Boundary {
    pub fn new(radius_m: f64, tolerance_m: f64) -> Result<Self> {
        ensure!(
            radius_m.is_finite() && radius_m > 0.,
            "radius must be positive and finite"
        );
        ensure!(
            tolerance_m.is_finite() && tolerance_m > 0. && tolerance_m < radius_m,
            "boundary tolerance must be positive and smaller than radius"
        );
        let angle = (1. - tolerance_m / radius_m).clamp(-1., 1.).acos();
        ensure!(
            angle > 0.,
            "boundary tolerance is below floating-point precision"
        );
        let n = (std::f64::consts::PI / angle).ceil().max(16.) as usize;
        ensure!(
            n <= 16384,
            "boundary tolerance needs more than 16384 segments; increase it"
        );
        let points = (0..n)
            .map(|i| {
                let a = std::f64::consts::TAU * i as f64 / n as f64;
                Vertex::new(a.cos(), a.sin(), 0.)
            })
            .collect();
        let inner = (std::f64::consts::PI / n as f64).cos();
        Ok(Self {
            points,
            radial_error_m: radius_m * (1. - inner),
            inner_squared: inner * inner,
        })
    }
    pub fn clip(&self, p: &[Vertex]) -> Polygon {
        if p.iter()
            .all(|v| v.x * v.x + v.y * v.y <= self.inner_squared)
        {
            return p.to_vec();
        }
        let (xmin, xmax, ymin, ymax) = bounds(p);
        let dx = if xmin > 0. {
            xmin
        } else if xmax < 0. {
            -xmax
        } else {
            0.
        };
        let dy = if ymin > 0. {
            ymin
        } else if ymax < 0. {
            -ymax
        } else {
            0.
        };
        if dx * dx + dy * dy > 1. {
            return vec![];
        }
        let mut q = p.to_vec();
        for i in 0..self.points.len() {
            let a = self.points[i];
            let b = self.points[(i + 1) % self.points.len()];
            q = clip(&q, |v| cross(a, b, v));
            if q.len() < 3 {
                return vec![];
            }
        }
        q
    }
}
pub fn bounds(p: &[Vertex]) -> (f64, f64, f64, f64) {
    p.iter().fold(
        (
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ),
        |b, v| (b.0.min(v.x), b.1.max(v.x), b.2.min(v.y), b.3.max(v.y)),
    )
}
pub fn snap_polygon(p: &mut Polygon) {
    if p.iter()
        .any(|v| !v.x.is_finite() || !v.y.is_finite() || v.x.abs() > 2. || v.y.abs() > 2.)
    {
        p.clear();
        return;
    }
    for v in p.iter_mut() {
        let k = v.key();
        v.x = k.0 as f64 / SNAP;
        v.y = k.1 as f64 / SNAP;
    }
    // Input is a convex clipping primitive. Rounding can make collinear edges
    // microscopically concave: canonicalize with exact integer orientation,
    // within the vertex-rounding bound. Never applied to concave terrain regions.
    p.sort_by_key(|v| v.key());
    p.dedup_by_key(|v| v.key());
    if p.len() < 3 {
        return;
    }
    let mut lower: Polygon = vec![];
    let mut upper: Polygon = vec![];
    for &v in p.iter() {
        while lower.len() >= 2
            && key_cross(
                lower[lower.len() - 2].key(),
                lower[lower.len() - 1].key(),
                v.key(),
            ) <= 0
        {
            lower.pop();
        }
        lower.push(v);
    }
    for &v in p.iter().rev() {
        while upper.len() >= 2
            && key_cross(
                upper[upper.len() - 2].key(),
                upper[upper.len() - 1].key(),
                v.key(),
            ) <= 0
        {
            upper.pop();
        }
        upper.push(v);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    *p = lower;
}
type PointKey = (i64, i64);
type Edge = (PointKey, PointKey);
fn edge_key(a: Vertex, b: Vertex) -> Edge {
    (a.key(), b.key())
}

/// Greedy, deterministic convex unions. Internal shared edges are removed only
/// when every remaining turn is convex. Holes cannot be filled by this operation.
pub fn merge_convex(input: Vec<Polygon>) -> Vec<Polygon> {
    let mut polys: Vec<Option<Polygon>> = input.into_iter().map(Some).collect();
    let mut edges: HashMap<Edge, usize> = HashMap::new();
    let mut queue = VecDeque::new();
    fn register(
        id: usize,
        p: &[Vertex],
        edges: &mut HashMap<Edge, usize>,
        q: &mut VecDeque<(usize, usize, Edge)>,
    ) {
        for i in 0..p.len() {
            let e = edge_key(p[i], p[(i + 1) % p.len()]);
            if let Some(&other) = edges.get(&(e.1, e.0)) {
                q.push_back((id, other, e));
            }
            edges.insert(e, id);
        }
    }
    for (id, p) in polys.iter().enumerate() {
        register(id, p.as_ref().unwrap(), &mut edges, &mut queue);
    }
    while let Some((a, b, e)) = queue.pop_front() {
        if a == b {
            continue;
        }
        let (Some(pa), Some(pb)) = (&polys[a], &polys[b]) else {
            continue;
        };
        let ia = (0..pa.len()).find(|&i| edge_key(pa[i], pa[(i + 1) % pa.len()]) == e);
        let ib = (0..pb.len()).find(|&i| edge_key(pb[i], pb[(i + 1) % pb.len()]) == (e.1, e.0));
        let (Some(ia), Some(ib)) = (ia, ib) else {
            continue;
        };
        if pa.len() + pb.len() > 130 {
            continue;
        }
        let mut joined = Vec::with_capacity(pa.len() + pb.len() - 2);
        for k in 1..=pa.len() {
            joined.push(pa[(ia + k) % pa.len()]);
        }
        for k in 2..pb.len() {
            joined.push(pb[(ib + k) % pb.len()]);
        }
        // Remove truly collinear points, preserving the exact boundary.
        loop {
            let redundant = (0..joined.len()).find(|&i| {
                let n = joined.len();
                key_cross(
                    joined[(i + n - 1) % n].key(),
                    joined[i].key(),
                    joined[(i + 1) % n].key(),
                ) == 0
            });
            if joined.len() <= 3 || redundant.is_none() {
                break;
            }
            joined.remove(redundant.unwrap());
        }
        if !is_convex(&joined) || joined.len() > 128 {
            continue;
        }
        let original_area = area(pa) + area(pb);
        if (area(&joined) - original_area).abs() > 1e-12 * original_area.max(1e-6) {
            continue;
        }
        for (id, p) in [(a, pa), (b, pb)] {
            for i in 0..p.len() {
                let k = edge_key(p[i], p[(i + 1) % p.len()]);
                if edges.get(&k) == Some(&id) {
                    edges.remove(&k);
                }
            }
        }
        polys[b] = None;
        register(a, &joined, &mut edges, &mut queue);
        polys[a] = Some(joined);
    }
    polys.into_iter().flatten().collect()
}
