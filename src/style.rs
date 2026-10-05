//! Deterministic solid color interpolation; positions span the retained range.
use anyhow::{Result, ensure};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum GradientMode {
    #[default]
    Srgb,
    LinearRgb,
    HsvShort,
    HsvLong,
    Rainbow,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ColorStop {
    pub position: f64,
    pub color: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ElevationColor {
    pub elevation_m: f64,
    pub color: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ElevationWidth {
    pub elevation_m: f64,
    pub width_px: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EveryNColor {
    pub starting_elevation_m: f64,
    pub every: usize,
    pub color: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EveryNWidth {
    pub starting_elevation_m: f64,
    pub every: usize,
    pub width_px: f64,
}
fn matches_pattern(start: f64, every: usize, index: usize, levels: &[f64]) -> bool {
    let anchor = levels.partition_point(|z| *z < start);
    every > 0 && index >= anchor && (index - anchor).is_multiple_of(every)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Gradient {
    pub lowest: String,
    pub highest: String,
    pub mode: GradientMode,
    pub stops: Vec<ColorStop>,
}
impl Default for Gradient {
    fn default() -> Self {
        Self {
            lowest: "#ffffff".into(),
            highest: "#000000".into(),
            mode: GradientMode::Srgb,
            stops: vec![],
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Style {
    pub fill: Gradient,
    pub stroke: Gradient,
    pub stroke_width_px: f64,
    pub specific_fill: Vec<ElevationColor>,
    pub specific_stroke_fill: Vec<ElevationColor>,
    pub specific_stroke_width: Vec<ElevationWidth>,
    #[serde(default)]
    pub no_fill: bool,
    #[serde(default)]
    pub no_stroke: bool,
    #[serde(default)]
    pub every_n_fill: Vec<EveryNColor>,
    #[serde(default)]
    pub every_n_stroke_fill: Vec<EveryNColor>,
    #[serde(default)]
    pub every_n_stroke_width: Vec<EveryNWidth>,
}
impl Default for Style {
    fn default() -> Self {
        Self {
            fill: Gradient::default(),
            stroke: Gradient {
                lowest: "#000000".into(),
                ..Gradient::default()
            },
            stroke_width_px: 0.,
            specific_fill: vec![],
            specific_stroke_fill: vec![],
            specific_stroke_width: vec![],
            no_fill: false,
            no_stroke: false,
            every_n_fill: vec![],
            every_n_stroke_fill: vec![],
            every_n_stroke_width: vec![],
        }
    }
}
pub fn parse_color(s: &str) -> Result<[u8; 3]> {
    let s = s.trim();
    ensure!(
        s.len() == 7 && s.starts_with('#') && s[1..].bytes().all(|b| b.is_ascii_hexdigit()),
        "color must be #RRGGBB"
    );
    Ok([
        u8::from_str_radix(&s[1..3], 16)?,
        u8::from_str_radix(&s[3..5], 16)?,
        u8::from_str_radix(&s[5..7], 16)?,
    ])
}
pub fn canonical_color(s: &str) -> Result<String> {
    Ok(hex(parse_color(s)?))
}
fn hex(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}
pub fn parse_elevation(s: &str) -> Result<f64> {
    let s = s.trim().to_ascii_lowercase();
    let (n, k) = if let Some(n) = s.strip_suffix("ft") {
        (n, 0.3048)
    } else if let Some(n) = s.strip_suffix("mi") {
        (n, 1609.344)
    } else if let Some(n) = s.strip_suffix("km") {
        (n, 1000.)
    } else if let Some(n) = s.strip_suffix('m') {
        (n, 1.)
    } else {
        (s.as_str(), 1.)
    };
    let z = n.trim().parse::<f64>()? * k;
    ensure!(z.is_finite(), "elevation must be finite");
    Ok(z)
}
pub fn parse_width(s: &str) -> Result<f64> {
    let s = s.trim();
    let w = s.strip_suffix("px").unwrap_or(s).trim().parse::<f64>()?;
    ensure!(
        w.is_finite() && w >= 0.,
        "stroke width must be finite and nonnegative (pixels)"
    );
    Ok(w)
}
impl Style {
    pub fn validate(&self) -> Result<()> {
        self.fill.validate()?;
        self.stroke.validate()?;
        ensure!(
            self.stroke_width_px.is_finite() && self.stroke_width_px >= 0.,
            "invalid stroke width"
        );
        for list in [&self.specific_fill, &self.specific_stroke_fill] {
            for (i, s) in list.iter().enumerate() {
                ensure!(
                    s.elevation_m.is_finite(),
                    "specific elevation must be finite"
                );
                parse_color(&s.color)?;
                ensure!(
                    !list[..i].iter().any(|v| v.elevation_m == s.elevation_m),
                    "duplicate specific elevation {} m",
                    s.elevation_m
                );
            }
        }
        for list in [&self.every_n_fill, &self.every_n_stroke_fill] {
            for pattern in list {
                ensure!(
                    pattern.starting_elevation_m.is_finite() && pattern.every > 0,
                    "pattern start must be finite and step count must be positive"
                );
                parse_color(&pattern.color)?;
            }
        }
        for pattern in &self.every_n_stroke_width {
            ensure!(
                pattern.starting_elevation_m.is_finite()
                    && pattern.every > 0
                    && pattern.width_px.is_finite()
                    && pattern.width_px >= 0.,
                "invalid every-N stroke width pattern"
            );
        }
        for (i, s) in self.specific_stroke_width.iter().enumerate() {
            ensure!(
                s.elevation_m.is_finite() && s.width_px.is_finite() && s.width_px >= 0.,
                "invalid specific stroke width"
            );
            ensure!(
                !self.specific_stroke_width[..i]
                    .iter()
                    .any(|v| v.elevation_m == s.elevation_m),
                "duplicate specific stroke width elevation"
            );
        }
        Ok(())
    }
    pub fn elevations(&self) -> impl Iterator<Item = f64> + '_ {
        self.specific_fill
            .iter()
            .chain(&self.specific_stroke_fill)
            .map(|s| s.elevation_m)
            .chain(self.specific_stroke_width.iter().map(|s| s.elevation_m))
    }
    pub fn at(&self, z: f64, t: f64, base_levels: &[f64]) -> (String, String, f64) {
        let mut fill = self.fill.at(t);
        let mut stroke = self.stroke.at(t);
        let mut width = self.stroke_width_px;
        if let Some(i) = base_levels.iter().position(|v| *v == z) {
            for p in &self.every_n_fill {
                if matches_pattern(p.starting_elevation_m, p.every, i, base_levels) {
                    fill = canonical_color(&p.color).unwrap();
                }
            }
            for p in &self.every_n_stroke_fill {
                if matches_pattern(p.starting_elevation_m, p.every, i, base_levels) {
                    stroke = canonical_color(&p.color).unwrap();
                }
            }
            for p in &self.every_n_stroke_width {
                if matches_pattern(p.starting_elevation_m, p.every, i, base_levels) {
                    width = p.width_px;
                }
            }
        }
        if let Some(s) = self.specific_fill.iter().find(|s| s.elevation_m == z) {
            fill = canonical_color(&s.color).unwrap();
        }
        if let Some(s) = self
            .specific_stroke_fill
            .iter()
            .find(|s| s.elevation_m == z)
        {
            stroke = canonical_color(&s.color).unwrap();
        }
        if let Some(s) = self
            .specific_stroke_width
            .iter()
            .find(|s| s.elevation_m == z)
        {
            width = s.width_px;
        }
        if self.no_fill {
            fill = "none".into();
        }
        if self.no_stroke {
            stroke = "none".into();
            width = 0.;
        }
        (fill, stroke, width)
    }
}
impl Gradient {
    pub fn validate(&self) -> Result<()> {
        parse_color(&self.lowest)?;
        parse_color(&self.highest)?;
        ensure!(
            !matches!(self.mode, GradientMode::Rainbow) || self.stops.is_empty(),
            "rainbow uses built-in stops; choose another mode for custom stops"
        );
        for (i, s) in self.stops.iter().enumerate() {
            ensure!(
                s.position.is_finite() && s.position > 0. && s.position < 1.,
                "gradient stop position must be between 0 and 1 (exclusive)"
            );
            parse_color(&s.color)?;
            ensure!(
                !self.stops[..i].iter().any(|v| v.position == s.position),
                "duplicate gradient stop position"
            );
        }
        Ok(())
    }
    /// Call after validation. Endpoint values are exact, including flat t=0.
    pub fn at(&self, t: f64) -> String {
        let t = t.clamp(0., 1.);
        let mut stops: Vec<(f64, [u8; 3])> = vec![
            (0., parse_color(&self.lowest).unwrap()),
            (1., parse_color(&self.highest).unwrap()),
        ];
        if matches!(self.mode, GradientMode::Rainbow) {
            for (i, c) in [
                "#ff0000", "#ffff00", "#00ff00", "#00ffff", "#0000ff", "#ff00ff",
            ]
            .iter()
            .enumerate()
            {
                stops.push(((i + 1) as f64 / 7., parse_color(c).unwrap()));
            }
        } else {
            stops.extend(
                self.stops
                    .iter()
                    .map(|s| (s.position, parse_color(&s.color).unwrap())),
            );
        }
        stops.sort_by(|a, b| a.0.total_cmp(&b.0));
        let i = stops.partition_point(|s| s.0 < t).clamp(1, stops.len() - 1);
        let (a, b) = (stops[i - 1], stops[i]);
        hex(interpolate(a.1, b.1, (t - a.0) / (b.0 - a.0), self.mode))
    }
}
fn interpolate(a: [u8; 3], b: [u8; 3], t: f64, mode: GradientMode) -> [u8; 3] {
    if t <= 0. {
        return a;
    }
    if t >= 1. {
        return b;
    }
    let a = a.map(|x| x as f64 / 255.);
    let b = b.map(|x| x as f64 / 255.);
    let c = match mode {
        GradientMode::LinearRgb => {
            std::array::from_fn(|i| encode(decode(a[i]) + (decode(b[i]) - decode(a[i])) * t))
        }
        GradientMode::HsvShort | GradientMode::HsvLong => {
            let mut a = hsv(a);
            let mut b = hsv(b);
            if a[1] == 0. {
                a[0] = b[0]
            }
            if b[1] == 0. {
                b[0] = a[0]
            }
            let mut d = (b[0] - a[0] + 0.5).rem_euclid(1.) - 0.5;
            if matches!(mode, GradientMode::HsvLong) && a[1] > 0. && b[1] > 0. {
                d = if d >= 0. { d - 1. } else { d + 1. };
            }
            rgb([
                (a[0] + d * t).rem_euclid(1.),
                a[1] + (b[1] - a[1]) * t,
                a[2] + (b[2] - a[2]) * t,
            ])
        }
        _ => std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t),
    };
    c.map(|v| (v.clamp(0., 1.) * 255.).round() as u8)
}
fn decode(c: f64) -> f64 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}
fn encode(c: f64) -> f64 {
    if c <= 0.0031308 {
        12.92 * c
    } else {
        1.055 * c.powf(1. / 2.4) - 0.055
    }
}
fn hsv(c: [f64; 3]) -> [f64; 3] {
    let max = c.into_iter().fold(0., f64::max);
    let min = c.into_iter().fold(1., f64::min);
    let d = max - min;
    let h = if d == 0. {
        0.
    } else if max == c[0] {
        ((c[1] - c[2]) / d).rem_euclid(6.) / 6.
    } else if max == c[1] {
        ((c[2] - c[0]) / d + 2.) / 6.
    } else {
        ((c[0] - c[1]) / d + 4.) / 6.
    };
    [h, if max == 0. { 0. } else { d / max }, max]
}
fn rgb(c: [f64; 3]) -> [f64; 3] {
    let h = c[0] * 6.;
    let chroma = c[2] * c[1];
    let x = chroma * (1. - (h.rem_euclid(2.) - 1.).abs());
    let m = c[2] - chroma;
    let p = match h.floor() as u8 {
        0 => [chroma, x, 0.],
        1 => [x, chroma, 0.],
        2 => [0., chroma, x],
        3 => [0., x, chroma],
        4 => [x, 0., chroma],
        _ => [chroma, 0., x],
    };
    p.map(|v| v + m)
}
