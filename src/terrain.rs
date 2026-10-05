use crate::geometry::{self, Boundary, Polygon, Vertex};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug)]
pub struct Grid {
    pub size: usize,
    pub values: Vec<f64>,
}
impl Grid {
    pub fn new(size: usize, values: Vec<f64>) -> Result<Self> {
        ensure!((3..=2049).contains(&size), "grid size must be 3..2049");
        ensure!(
            values.len() == size * size,
            "grid dimensions do not match data"
        );
        Ok(Self { size, values })
    }
    pub fn analytic(size: usize, f: impl Fn(f64, f64) -> f64) -> Result<Self> {
        ensure!((3..=2049).contains(&size), "grid size must be 3..2049");
        let mut values = Vec::with_capacity(size * size);
        for row in 0..size {
            for col in 0..size {
                values.push(f(
                    -1. + 2. * col as f64 / (size - 1) as f64,
                    1. - 2. * row as f64 / (size - 1) as f64,
                ));
            }
        }
        Self::new(size, values)
    }
    pub fn vertex(&self, row: usize, col: usize) -> Vertex {
        Vertex::new(
            -1. + 2. * col as f64 / (self.size - 1) as f64,
            1. - 2. * row as f64 / (self.size - 1) as f64,
            self.values[row * self.size + col],
        )
    }
    pub fn triangles(&self) -> impl Iterator<Item = [Vertex; 3]> + '_ {
        (0..self.size - 1)
            .flat_map(move |r| {
                (0..self.size - 1).flat_map(move |c| {
                    let a = self.vertex(r, c);
                    let b = self.vertex(r + 1, c);
                    let d = self.vertex(r, c + 1);
                    let e = self.vertex(r + 1, c + 1);
                    [[a, b, e], [a, e, d]]
                })
            })
            .filter(|t| t.iter().all(|v| v.z.is_finite()))
    }
    pub fn sample(&self, x: f64, y: f64) -> Option<f64> {
        if !(-1. ..=1.).contains(&x) || !(-1. ..=1.).contains(&y) {
            return None;
        }
        let u = (x + 1.) * (self.size - 1) as f64 / 2.;
        let v = (1. - y) * (self.size - 1) as f64 / 2.;
        let c = (u.floor() as usize).min(self.size - 2);
        let r = (v.floor() as usize).min(self.size - 2);
        let u = u - c as f64;
        let v = v - r as f64;
        let a = self.values[r * self.size + c];
        let e = self.values[(r + 1) * self.size + c + 1];
        let (vertices, weights) = if v >= u {
            (
                [a, self.values[(r + 1) * self.size + c], e],
                [1. - v, v - u, u],
            )
        } else {
            (
                [a, e, self.values[r * self.size + c + 1]],
                [1. - u, v, u - v],
            )
        };
        vertices.iter().all(|z| z.is_finite()).then(|| {
            vertices[0]
                + weights[1] * (vertices[1] - vertices[0])
                + weights[2] * (vertices[2] - vertices[0])
        })
    }
}

/// Inclusive physical elevation bounds, in the input raster's vertical datum.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct ElevationFilter {
    pub minimum_m: Option<f64>,
    pub maximum_m: Option<f64>,
}
impl ElevationFilter {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.minimum_m.is_none_or(f64::is_finite) && self.maximum_m.is_none_or(f64::is_finite),
            "elevation limits must be finite"
        );
        if let (Some(a), Some(b)) = (self.minimum_m, self.maximum_m) {
            ensure!(
                a <= b,
                "minimum elevation must not exceed maximum elevation"
            );
        }
        Ok(())
    }
    pub fn includes(&self, z: f64) -> bool {
        self.minimum_m.is_none_or(|m| z >= m) && self.maximum_m.is_none_or(|m| z <= m)
    }
    fn clip(&self, mut p: Polygon) -> Polygon {
        if let Some(m) = self.minimum_m {
            p = geometry::clip(&p, |v| v.z - m);
        }
        if let Some(m) = self.maximum_m {
            p = geometry::clip(&p, |v| m - v.z);
        }
        // Enforce exact height limits after interpolation roundoff; XY remains
        // the original intersection, subject to the reported spatial tolerance.
        for v in &mut p {
            if let Some(m) = self.minimum_m {
                v.z = v.z.max(m);
            }
            if let Some(m) = self.maximum_m {
                v.z = v.z.min(m);
            }
        }
        p
    }
}

#[derive(Clone, Debug)]
pub struct Settings {
    pub radius_m: f64,
    pub boundary_tolerance_m: f64,
    pub level_count: usize,
    pub interval_m: Option<f64>,
    pub color_range: Option<(f64, f64)>,
    pub elevation_filter: ElevationFilter,
    pub style: crate::style::Style,
    pub merge: bool,
    pub max_polygons: usize,
    pub audit_samples: usize,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            radius_m: 1000.,
            boundary_tolerance_m: 1.,
            level_count: 64,
            interval_m: None,
            color_range: None,
            elevation_filter: ElevationFilter::default(),
            style: crate::style::Style::default(),
            merge: true,
            max_polygons: 2_000_000,
            audit_samples: 4096,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Level {
    pub elevation_m: f64,
    #[serde(default)]
    pub base_level_index: Option<usize>,
    pub band_min_m: f64,
    pub band_max_m: f64,
    pub gray: u8,
    pub fill: String,
    pub stroke_fill: String,
    pub stroke_width_px: f64,
    pub polygon_count: usize,
    pub area_m2: f64,
    pub union_component_count: usize,
    pub union_ring_count: usize,
    pub footprint_component_count: usize,
    pub footprint_ring_count: usize,
}
#[derive(Clone, Debug)]
pub struct Layer {
    pub level: Level,
    pub polygons: Vec<Polygon>,
    pub union: Vec<crate::union::Shape>,
    pub footprint: Vec<crate::union::Shape>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Accuracy {
    pub coverage_fraction: f64,
    pub covered_area_m2: f64,
    pub elevation_filter: ElevationFilter,
    pub retained_fraction: f64,
    pub retained_area_m2: f64,
    pub minimum_elevation_m: f64,
    pub maximum_elevation_m: f64,
    pub maximum_level_gap_m: f64,
    pub quantization_bound_m: f64,
    pub boundary_segments: usize,
    pub boundary_radial_error_m: f64,
    pub coordinate_rounding_bound_m: f64,
    pub decomposition_area_change_m2: f64,
    pub rounding_area_change_m2: f64,
    pub union_area_change_m2: f64,
    pub union_coordinate_grid_m: f64,
    pub maximum_footprint_area_change_m2: f64,
    pub audit_attempts: usize,
    pub audit_samples: usize,
    pub audit_missing: usize,
    pub audit_position_tolerance_m: f64,
    pub audit_boundary_excluded: usize,
    pub audit_source_nodata: usize,
    pub audit_elevation_excluded: usize,
    pub audit_bias_m: Option<f64>,
    pub audit_rmse_m: Option<f64>,
    pub audit_max_abs_error_m: Option<f64>,
    pub working_spacing_m: f64,
    pub grid_size: usize,
    pub polygon_count: usize,
    pub raw_piece_count: usize,
    pub color_range_m: [f64; 2],
    pub interpretation: String,
}
#[derive(Clone, Debug)]
pub struct Terrain {
    pub layers: Vec<Layer>,
    pub accuracy: Accuracy,
    pub style: crate::style::Style,
}

pub fn base_elevations(min: f64, max: f64, s: &Settings) -> Result<Vec<f64>> {
    s.elevation_filter.validate()?;
    s.style.validate()?;
    ensure!(
        min.is_finite() && max.is_finite() && min <= max,
        "invalid elevation range"
    );
    ensure!(
        (max - min).is_finite(),
        "elevation range overflows floating-point representation"
    );
    ensure!(
        (2..=1024).contains(&s.level_count),
        "level count must be 2..1024"
    );
    if let Some((a, b)) = s.color_range {
        ensure!(
            a.is_finite() && b.is_finite() && a < b && (b - a).is_finite(),
            "color range must have finite minimum < maximum"
        );
    }
    if let Some(step) = s.interval_m {
        ensure!(
            step.is_finite() && step > 0.,
            "elevation interval must be positive and finite"
        );
    }
    if min == max {
        return Ok(vec![min]);
    }
    let heights: Vec<f64> = if let Some(step) = s.interval_m {
        let count = ((max - min) / step).ceil();
        ensure!(
            count <= 1023.,
            "interval creates more than 1024 levels; increase it"
        );
        let mut out = vec![min];
        for i in 1..count as usize {
            out.push(min + step * i as f64);
        }
        out.push(max);
        out
    } else {
        (0..s.level_count)
            .map(|i| {
                if i == s.level_count - 1 {
                    max
                } else {
                    min + (max - min) * (i as f64 / (s.level_count - 1) as f64)
                }
            })
            .collect()
    };
    ensure!(
        heights.windows(2).all(|w| w[0] < w[1]),
        "elevation steps are below floating-point precision; use fewer levels or a larger interval"
    );
    Ok(heights)
}

pub fn elevations(min: f64, max: f64, s: &Settings) -> Result<Vec<f64>> {
    let mut heights = base_elevations(min, max, s)?;
    heights.extend(s.style.elevations().filter(|z| *z >= min && *z <= max));
    heights.sort_by(f64::total_cmp);
    heights.dedup();
    ensure!(
        heights.len() <= 1024,
        "specific elevations create more than 1024 levels"
    );
    Ok(heights)
}
pub fn grayscale(z: f64, min: f64, max: f64) -> u8 {
    if min == max {
        255
    } else {
        (255. * (1. - ((z - min) / (max - min)).clamp(0., 1.))).round() as u8
    }
}
pub fn classify(z: f64, limits: &[f64]) -> usize {
    limits.partition_point(|&b| z >= b)
}

pub fn build(grid: &Grid, s: &Settings) -> Result<Terrain> {
    ensure!(
        (3..=2049).contains(&grid.size) && grid.values.len() == grid.size * grid.size,
        "invalid working grid"
    );
    ensure!(s.max_polygons > 0, "polygon budget must be positive");
    ensure!(
        s.audit_samples <= 1_000_000,
        "audit sample budget exceeds 1000000"
    );
    s.elevation_filter.validate()?;
    s.style.validate()?;
    let boundary = Boundary::new(s.radius_m, s.boundary_tolerance_m)?;
    let mut source_area = 0.;
    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    // Extrema include intersections with the boundary, not outside-grid peaks.
    for t in grid.triangles() {
        let p = boundary.clip(&t);
        source_area += geometry::area(&p);
        let p = s.elevation_filter.clip(p);
        if geometry::area(&p) <= 1e-20 {
            continue;
        }
        for v in p {
            min = min.min(v.z);
            max = max.max(v.z);
        }
    }
    ensure!(source_area > 1e-20, "no valid terrain inside the circle");
    ensure!(
        min.is_finite(),
        "no terrain remains within requested elevation limits"
    );
    let base_heights = base_elevations(min, max, s)?;
    let heights = elevations(min, max, s)?;
    ensure!(
        heights.windows(2).all(|w| w[0] < w[1]),
        "elevation steps are below floating-point precision; use fewer levels or a larger interval"
    );
    let limits: Vec<_> = heights
        .windows(2)
        .map(|w| w[0] + (w[1] - w[0]) / 2.)
        .collect();
    ensure!(
        limits
            .iter()
            .enumerate()
            .all(|(i, &b)| heights[i] < b && b < heights[i + 1]),
        "elevation band boundaries are below floating-point precision"
    );
    let (color_min, color_max) = s.color_range.unwrap_or((min, max));
    let mut layers: Vec<_> = heights
        .iter()
        .enumerate()
        .map(|(i, &z)| {
            let gray = grayscale(z, color_min, color_max);
            let t = if color_min == color_max {
                0.
            } else {
                ((z - color_min) / (color_max - color_min)).clamp(0., 1.)
            };
            let (fill, stroke_fill, stroke_width_px) = s.style.at(z, t, &base_heights);
            Layer {
                level: Level {
                    elevation_m: z,
                    base_level_index: base_heights.iter().position(|v| *v == z),
                    band_min_m: if i == 0 { min } else { limits[i - 1] },
                    band_max_m: limits.get(i).copied().unwrap_or(max),
                    gray,
                    fill,
                    stroke_fill,
                    stroke_width_px,
                    polygon_count: 0,
                    area_m2: 0.,
                    union_component_count: 0,
                    union_ring_count: 0,
                    footprint_component_count: 0,
                    footprint_ring_count: 0,
                },
                polygons: vec![],
                union: vec![],
                footprint: vec![],
            }
        })
        .collect();
    let mut piece_count = 0;
    let mut before = 0.;
    let mut exact_area = 0.;
    for t in grid.triangles() {
        let p = s.elevation_filter.clip(boundary.clip(&t));
        let a = geometry::area(&p);
        if a <= 1e-20 {
            continue;
        }
        exact_area += a;
        let lo = p.iter().map(|v| v.z).fold(f64::INFINITY, f64::min);
        let hi = p.iter().map(|v| v.z).fold(f64::NEG_INFINITY, f64::max);
        for (i, layer) in layers
            .iter_mut()
            .enumerate()
            .take(classify(hi, &limits) + 1)
            .skip(classify(lo, &limits))
        {
            let mut q = p.clone();
            if i > 0 {
                q = geometry::clip(&q, |v| v.z - limits[i - 1]);
            }
            if i < limits.len() {
                q = geometry::clip(&q, |v| limits[i] - v.z);
            }
            if geometry::area(&q) <= 1e-20 {
                continue;
            }
            geometry::snap_polygon(&mut q);
            if q.len() < 3 || geometry::area(&q) <= 1e-20 {
                continue;
            }
            ensure!(
                geometry::is_convex(&q),
                "numerical clipping produced a non-convex piece; increase detail tolerances"
            );
            piece_count += 1;
            ensure!(
                piece_count <= s.max_polygons,
                "raw polygon budget exceeded ({}); reduce grid size or elevation levels",
                s.max_polygons
            );
            before += geometry::area(&q);
            layer.polygons.push(q);
        }
    }
    let mut after = 0.;
    let mut polygon_count = 0;
    let mut union_area_change = 0.;
    for layer in &mut layers {
        if s.merge {
            layer.polygons = geometry::merge_convex(std::mem::take(&mut layer.polygons));
        }
        layer.level.polygon_count = layer.polygons.len();
        polygon_count += layer.polygons.len();
        let a: f64 = layer.polygons.iter().map(|p| geometry::area(p)).sum();
        after += a;
        layer.union = crate::union::union(&layer.polygons, layer.level.elevation_m)?;
        let union_area: f64 = layer.union.iter().map(|p| p.area()).sum();
        union_area_change += (union_area - a).abs();
        layer.level.area_m2 = union_area * s.radius_m * s.radius_m;
        layer.level.union_component_count = layer.union.len();
        layer.level.union_ring_count = layer.union.iter().map(|s| s.rings.len()).sum();
    }
    // Fill each level's superlevel footprint: lower terrain lies underneath
    // higher terrain, so antialiasing never reveals background at band joins.
    let mut higher = vec![];
    let mut expected_area = 0.;
    let mut footprint_area_change: f64 = 0.;
    for layer in layers.iter_mut().rev() {
        expected_area += layer.union.iter().map(|s| s.area()).sum::<f64>();
        higher.extend(layer.union.iter().cloned());
        layer.footprint = crate::union::union_shapes(&higher, layer.level.elevation_m)?;
        let actual_area: f64 = layer.footprint.iter().map(|s| s.area()).sum();
        footprint_area_change = footprint_area_change.max((actual_area - expected_area).abs());
        layer.level.footprint_component_count = layer.footprint.len();
        layer.level.footprint_ring_count = layer.footprint.iter().map(|s| s.rings.len()).sum();
        higher = layer.footprint.clone();
    }
    let gap = heights.windows(2).map(|w| w[1] - w[0]).fold(0., f64::max);
    let mut terrain=Terrain{layers,style:s.style.clone(),accuracy:Accuracy {
        coverage_fraction:source_area/std::f64::consts::PI,covered_area_m2:source_area*s.radius_m*s.radius_m,
        elevation_filter:s.elevation_filter,retained_fraction:exact_area/std::f64::consts::PI,retained_area_m2:exact_area*s.radius_m*s.radius_m,
        minimum_elevation_m:min,maximum_elevation_m:max,maximum_level_gap_m:gap,quantization_bound_m:gap/2.,
        boundary_segments:boundary.points.len(),boundary_radial_error_m:boundary.radial_error_m,
        coordinate_rounding_bound_m:2_f64.sqrt()*0.5/geometry::SNAP*s.radius_m,
        decomposition_area_change_m2:(after-before).abs()*s.radius_m*s.radius_m,
        maximum_footprint_area_change_m2:footprint_area_change*s.radius_m*s.radius_m,
        union_area_change_m2:union_area_change*s.radius_m*s.radius_m,union_coordinate_grid_m:s.radius_m/geometry::SNAP,
        rounding_area_change_m2:(before-exact_area).abs()*s.radius_m*s.radius_m,
        audit_attempts:s.audit_samples,audit_samples:0,audit_missing:0,audit_position_tolerance_m:s.radius_m/geometry::SNAP,audit_boundary_excluded:0,audit_source_nodata:0,audit_elevation_excluded:0,audit_bias_m:None,audit_rmse_m:None,audit_max_abs_error_m:None,
        working_spacing_m:2.*s.radius_m/(grid.size-1) as f64,grid_size:grid.size,polygon_count,
        raw_piece_count:piece_count,color_range_m:[color_min,color_max],
        interpretation:"Residuals compare exported unioned elevation bands with the piecewise-planar working grid, not independently measured Earth elevations. Source uncertainty and resampling error are separate; this report does not establish either.".into(),
    }};
    audit(&mut terrain, grid, &boundary, s.audit_samples);
    ensure!(
        terrain.accuracy.audit_missing == 0,
        "unioned geometry failed coverage audit ({} valid terrain points missing)",
        terrain.accuracy.audit_missing
    );
    Ok(terrain)
}

/// Spatial buckets reference the final union shapes rendered by the SVG.
pub struct Index<'a> {
    terrain: &'a Terrain,
    buckets: Vec<Vec<(usize, usize)>>,
}
impl<'a> Index<'a> {
    fn bucket(x: f64) -> usize {
        (((x + 1.) * 32.).floor() as isize).clamp(0, 63) as usize
    }
    pub fn new(terrain: &'a Terrain) -> Self {
        let mut buckets = vec![vec![]; 4096];
        for (l, layer) in terrain.layers.iter().enumerate() {
            for (i, p) in layer.footprint.iter().enumerate() {
                let (a, b, c, d) = p.bounds();
                for y in
                    Self::bucket(c - 1. / geometry::SNAP)..=Self::bucket(d + 1. / geometry::SNAP)
                {
                    for x in Self::bucket(a - 1. / geometry::SNAP)
                        ..=Self::bucket(b + 1. / geometry::SNAP)
                    {
                        buckets[y * 64 + x].push((l, i));
                    }
                }
            }
        }
        Self { terrain, buckets }
    }
    pub fn elevation(&self, x: f64, y: f64) -> Option<f64> {
        self.buckets[Self::bucket(y) * 64 + Self::bucket(x)]
            .iter()
            .rev()
            .find_map(|&(l, i)| {
                let layer = &self.terrain.layers[l];
                layer.footprint[i]
                    .contains(x, y)
                    .then_some(layer.level.elevation_m)
            })
    }
}
fn audit(terrain: &mut Terrain, grid: &Grid, boundary: &Boundary, count: usize) {
    let index = Index::new(terrain);
    let mut n = 0;
    let mut missing = 0;
    let mut sum = 0.;
    let mut sum2 = 0.;
    let mut max: f64 = 0.;
    let mut boundary_excluded = 0;
    let mut nodata = 0;
    let mut elevation_excluded = 0;
    // Equal-area disk samples, deterministic irrational angular increment.
    for i in 0..count {
        let r = ((i as f64 + 0.5) / count as f64).sqrt();
        let a = i as f64 * 2.399963229728653;
        let (x, y) = (r * a.cos(), r * a.sin());
        if !geometry::contains(&boundary.points, x, y) {
            boundary_excluded += 1;
            continue;
        }
        if let Some(expected) = grid.sample(x, y) {
            if !terrain.accuracy.elevation_filter.includes(expected) {
                elevation_excluded += 1;
                continue;
            }
            if let Some(actual) = index.elevation(x, y) {
                let e = actual - expected;
                sum += e;
                sum2 += e * e;
                max = max.max(e.abs());
                n += 1;
            } else {
                missing += 1;
            }
        } else {
            nodata += 1;
        }
    }
    terrain.accuracy.audit_samples = n;
    terrain.accuracy.audit_missing = missing;
    terrain.accuracy.audit_boundary_excluded = boundary_excluded;
    terrain.accuracy.audit_source_nodata = nodata;
    terrain.accuracy.audit_elevation_excluded = elevation_excluded;
    if n > 0 {
        terrain.accuracy.audit_bias_m = Some(sum / n as f64);
        terrain.accuracy.audit_rmse_m = Some((sum2 / n as f64).sqrt());
        terrain.accuracy.audit_max_abs_error_m = Some(max);
    }
}
