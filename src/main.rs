use anyhow::{Context, Result, ensure};
use clap::{Parser, ValueEnum};
use coord2svg::{
    output::{self, Report},
    raster,
    sources::{self, Asset, Cache, Product, Region},
    style::{
        self, ColorStop, ElevationColor, ElevationWidth, EveryNColor, EveryNWidth, Gradient,
        GradientMode, Style,
    },
    terrain::{self, ElevationFilter, Grid, Settings},
};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, ValueEnum, PartialEq)]
enum Source {
    Auto,
    Etopo,
    Local,
}
#[derive(Parser, Debug)]
#[command(
    version,
    about = "Official land and seabed elevation to unioned contour SVG",
    long_about = "Generate a circular, unioned elevation SVG and an accompanying numeric JSON report. Radius supports 1–1000 km. The default online source is NOAA ETOPO 2022 v1; local rasters are also supported. Detail is independent of radius; native data resolution is not a promise of map accuracy."
)]
struct Args {
    #[arg(long, allow_hyphen_values = true)]
    lat: f64,
    #[arg(long, allow_hyphen_values = true)]
    lon: f64,
    /// Radius: metres by default, or a suffix such as 500km.
    #[arg(long,value_parser=distance)]
    radius: f64,
    /// Explicit projected sampling spacing, e.g. 1km. Maximum grid is 2049².
    #[arg(long,value_parser=distance)]
    resolution: Option<f64>,
    /// Number of sample nodes per axis. Default spacing is diameter / 512.
    #[arg(long, default_value_t = 513, conflicts_with = "resolution")]
    grid_size: usize,
    #[arg(long, default_value_t = 64, conflicts_with = "interval")]
    levels: usize,
    #[arg(long,value_parser=distance)]
    interval: Option<f64>,
    #[arg(long,value_parser=distance)]
    boundary_tolerance: Option<f64>,
    /// Fixed sRGB scale, MIN:MAX in metres; e.g. --color-range=-8000:4500.
    #[arg(long,value_parser=color_range,allow_hyphen_values=true)]
    color_range: Option<(f64, f64)>,
    /// Include terrain at or above this height: signed metres, km or international ft.
    #[arg(long, value_parser=elevation, allow_hyphen_values=true)]
    min_elevation: Option<f64>,
    /// Include terrain at or below this height: e.g. 10000ft.
    #[arg(long, value_parser=elevation, allow_hyphen_values=true)]
    max_elevation: Option<f64>,
    /// Lowest retained elevation's color (quote shell # characters).
    #[arg(long, default_value="#ffffff", value_parser=hex_color)]
    lowest_fill: String,
    #[arg(long, default_value="#000000", value_parser=hex_color)]
    highest_fill: String,
    #[arg(long, value_enum, default_value = "srgb")]
    fill_gradient: GradientMode,
    /// Repeat POSITION,#RRGGBB stops, with positions strictly between 0 and 1.
    #[arg(long, value_parser=color_stop)]
    fill_stop: Vec<ColorStop>,
    #[arg(long, value_parser=hex_color, conflicts_with_all=["lowest_stroke_fill", "highest_stroke_fill", "stroke_stop", "stroke_gradient"])]
    stroke_fill: Option<String>,
    #[arg(long, value_parser=hex_color)]
    lowest_stroke_fill: Option<String>,
    #[arg(long, value_parser=hex_color)]
    highest_stroke_fill: Option<String>,
    #[arg(long, value_enum, default_value = "srgb")]
    stroke_gradient: GradientMode,
    #[arg(long, value_parser=color_stop)]
    stroke_stop: Vec<ColorStop>,
    /// Screen pixels: 0 disables strokes. Default 1px when a stroke color is set.
    #[arg(long, value_parser=stroke_width)]
    stroke_width: Option<f64>,
    /// Repeat HEIGHT,#RRGGBB; adds an in-range representative elevation group.
    #[arg(long, value_parser=elevation_color, allow_hyphen_values=true)]
    specific_fill: Vec<ElevationColor>,
    #[arg(long, value_parser=elevation_color, allow_hyphen_values=true)]
    specific_stroke_fill: Vec<ElevationColor>,
    /// Repeat HEIGHT,WIDTHpx; 0px disables that group's strokes.
    #[arg(long, value_parser=elevation_width, allow_hyphen_values=true)]
    specific_stroke_width: Vec<ElevationWidth>,
    /// Disable all fills, including pattern and specific overrides.
    #[arg(long)]
    no_fill: bool,
    /// Disable all strokes, including pattern and specific overrides.
    #[arg(long)]
    no_stroke: bool,
    /// Repeat START_HEIGHT,N,#RRGGBB on existing base levels; first at/above start.
    #[arg(long, value_parser=every_n_color, allow_hyphen_values=true)]
    every_n_fill: Vec<EveryNColor>,
    #[arg(long, value_parser=every_n_color, allow_hyphen_values=true)]
    every_n_stroke_fill: Vec<EveryNColor>,
    /// Repeat START_HEIGHT,N,WIDTHpx; specific overrides take precedence.
    #[arg(long, value_parser=every_n_width, allow_hyphen_values=true)]
    every_n_stroke_width: Vec<EveryNWidth>,
    #[arg(long, default_value = "terrain.svg")]
    out: PathBuf,
    #[arg(long, default_value_t = 1024)]
    width: u32,
    #[arg(long, value_enum, default_value = "auto")]
    source: Source,
    /// Repeat for compatible local rasters, in increasing priority order.
    #[arg(long)]
    input: Vec<PathBuf>,
    /// Explicit vertical reference for local metre-valued inputs. No vertical conversion is inferred.
    #[arg(long, requires = "input")]
    input_datum: Option<String>,
    /// Use a synthetic island/seabed fixture without downloading data.
    #[arg(long, conflicts_with = "input")]
    demo: bool,
    #[arg(long, default_value = ".cache/coord2svg")]
    cache_dir: PathBuf,
    #[arg(long)]
    offline: bool,
    #[arg(long, default_value_t = 512)]
    max_download_mb: u64,
    #[arg(long, default_value_t = 2_000_000)]
    max_polygons: usize,
    #[arg(long, default_value_t = 4096)]
    audit_samples: usize,
    /// Fail if valid covered area / ideal circle area is below this fraction.
    #[arg(long, default_value_t = 0.)]
    min_coverage: f64,
    #[arg(long)]
    no_merge: bool,
    #[arg(long)]
    save_grid: Option<PathBuf>,
    /// Print the data/resource plan as JSON. No downloads or output writes.
    #[arg(long)]
    dry_run: bool,
    #[arg(long)]
    force: bool,
}
fn distance(text: &str) -> std::result::Result<f64, String> {
    let text = text.trim();
    let (number, multiplier) = if let Some(v) = text.strip_suffix("km") {
        (v, 1000.)
    } else if let Some(v) = text.strip_suffix('m') {
        (v, 1.)
    } else {
        (text, 1.)
    };
    let value = number
        .trim()
        .parse::<f64>()
        .map_err(|_| "expected a number in metres or with m/km suffix".to_string())?
        * multiplier;
    if value.is_finite() && value > 0. {
        Ok(value)
    } else {
        Err("distance must be positive and finite".into())
    }
}
fn elevation(text: &str) -> std::result::Result<f64, String> {
    style::parse_elevation(text).map_err(|e| e.to_string())
}
fn hex_color(text: &str) -> std::result::Result<String, String> {
    style::canonical_color(text).map_err(|e| e.to_string())
}
fn stroke_width(text: &str) -> std::result::Result<f64, String> {
    style::parse_width(text).map_err(|e| e.to_string())
}
fn color_stop(text: &str) -> std::result::Result<ColorStop, String> {
    let (position, color) = text.split_once(',').ok_or("expected POSITION,#RRGGBB")?;
    let position = position
        .trim()
        .parse::<f64>()
        .map_err(|_| "invalid stop position")?;
    if !position.is_finite() || position <= 0. || position >= 1. {
        return Err("stop position must be between 0 and 1 (exclusive)".into());
    }
    Ok(ColorStop {
        position,
        color: hex_color(color)?,
    })
}
fn elevation_color(text: &str) -> std::result::Result<ElevationColor, String> {
    let (height, color) = text.split_once(',').ok_or("expected HEIGHT,#RRGGBB")?;
    Ok(ElevationColor {
        elevation_m: elevation(height)?,
        color: hex_color(color)?,
    })
}
fn elevation_width(text: &str) -> std::result::Result<ElevationWidth, String> {
    let (height, width) = text.split_once(',').ok_or("expected HEIGHT,WIDTHpx")?;
    Ok(ElevationWidth {
        elevation_m: elevation(height)?,
        width_px: stroke_width(width)?,
    })
}
fn pattern_parts(text: &str) -> std::result::Result<(f64, usize, &str), String> {
    let mut parts = text.split(',');
    let start = parts.next().ok_or("expected HEIGHT,N,STYLE")?;
    let every = parts
        .next()
        .ok_or("expected HEIGHT,N,STYLE")?
        .trim()
        .parse::<usize>()
        .map_err(|_| "N must be a positive integer")?;
    let value = parts.next().ok_or("expected HEIGHT,N,STYLE")?;
    if every == 0 || parts.next().is_some() {
        return Err("expected HEIGHT,N,STYLE with positive integer N".into());
    }
    Ok((elevation(start)?, every, value))
}
fn every_n_color(text: &str) -> std::result::Result<EveryNColor, String> {
    let (starting_elevation_m, every, color) = pattern_parts(text)?;
    Ok(EveryNColor {
        starting_elevation_m,
        every,
        color: hex_color(color)?,
    })
}
fn every_n_width(text: &str) -> std::result::Result<EveryNWidth, String> {
    let (starting_elevation_m, every, width) = pattern_parts(text)?;
    Ok(EveryNWidth {
        starting_elevation_m,
        every,
        width_px: stroke_width(width)?,
    })
}
fn color_range(text: &str) -> std::result::Result<(f64, f64), String> {
    let (a, b) = text.split_once(':').ok_or("expected MIN:MAX")?;
    let a = a.parse::<f64>().map_err(|_| "invalid color minimum")?;
    let b = b.parse::<f64>().map_err(|_| "invalid color maximum")?;
    if a.is_finite() && b.is_finite() && a < b {
        Ok((a, b))
    } else {
        Err("color range must be finite with MIN < MAX".into())
    }
}
fn main() -> Result<()> {
    run(Args::parse())
}
fn run(args: Args) -> Result<()> {
    let region = Region {
        latitude: args.lat,
        longitude: args.lon,
        radius_m: args.radius,
    };
    region.validate()?;
    ensure!(
        args.width > 0 && args.width <= 65536,
        "width must be 1..65536"
    );
    ensure!(
        args.min_coverage.is_finite() && (0. ..=1.).contains(&args.min_coverage),
        "minimum coverage must be 0..1"
    );
    let size = if let Some(res) = args.resolution {
        ((2. * args.radius / res).ceil() as usize)
            .saturating_add(1)
            .max(3)
    } else {
        args.grid_size
    };
    ensure!(
        (3..=2049).contains(&size),
        "requested sampling needs a {size}² grid; use a coarser resolution (maximum 2049²)"
    );
    let has_stroke = args.stroke_fill.is_some()
        || args.lowest_stroke_fill.is_some()
        || args.highest_stroke_fill.is_some()
        || !args.stroke_stop.is_empty()
        || !matches!(args.stroke_gradient, GradientMode::Srgb);
    let mut specific_stroke_width = args.specific_stroke_width;
    if !has_stroke && args.stroke_width.is_none() {
        for entry in &args.specific_stroke_fill {
            if !specific_stroke_width
                .iter()
                .any(|w| w.elevation_m == entry.elevation_m)
            {
                specific_stroke_width.push(ElevationWidth {
                    elevation_m: entry.elevation_m,
                    width_px: 1.,
                });
            }
        }
    }
    let mut every_n_stroke_width = vec![];
    if !has_stroke && args.stroke_width.is_none() {
        for p in &args.every_n_stroke_fill {
            every_n_stroke_width.push(EveryNWidth {
                starting_elevation_m: p.starting_elevation_m,
                every: p.every,
                width_px: 1.,
            });
        }
    }
    every_n_stroke_width.extend(args.every_n_stroke_width);
    let style = Style {
        fill: Gradient {
            lowest: args.lowest_fill,
            highest: args.highest_fill,
            mode: args.fill_gradient,
            stops: args.fill_stop,
        },
        stroke: Gradient {
            lowest: args
                .stroke_fill
                .clone()
                .or(args.lowest_stroke_fill)
                .unwrap_or("#000000".into()),
            highest: args
                .stroke_fill
                .or(args.highest_stroke_fill)
                .unwrap_or("#000000".into()),
            mode: args.stroke_gradient,
            stops: args.stroke_stop,
        },
        stroke_width_px: args
            .stroke_width
            .unwrap_or(if has_stroke { 1. } else { 0. }),
        specific_fill: args.specific_fill,
        specific_stroke_fill: args.specific_stroke_fill,
        specific_stroke_width,
        no_fill: args.no_fill,
        no_stroke: args.no_stroke,
        every_n_fill: args.every_n_fill,
        every_n_stroke_fill: args.every_n_stroke_fill,
        every_n_stroke_width,
    };
    let settings = Settings {
        radius_m: args.radius,
        boundary_tolerance_m: args
            .boundary_tolerance
            .unwrap_or((args.radius * 1e-5).max(0.05)),
        level_count: args.levels,
        interval_m: args.interval,
        color_range: args.color_range,
        elevation_filter: ElevationFilter {
            minimum_m: args.min_elevation,
            maximum_m: args.max_elevation,
        },
        style,
        merge: !args.no_merge,
        max_polygons: args.max_polygons,
        audit_samples: args.audit_samples,
    };
    // Validate all detail settings before network access, including flat input cases.
    terrain::elevations(0., 1., &settings)?;
    coord2svg::geometry::Boundary::new(args.radius, settings.boundary_tolerance_m)?;
    ensure!(
        args.max_polygons > 0 && args.audit_samples <= 1_000_000,
        "invalid polygon/audit budget"
    );
    ensure!(
        args.max_download_mb <= 16_384,
        "download budget cannot exceed 16384 MiB"
    );
    let use_etopo = !args.demo
        && (args.source == Source::Etopo || (args.source == Source::Auto && args.input.is_empty()));
    ensure!(
        args.demo || use_etopo || !args.input.is_empty(),
        "local source requires --input"
    );
    let datum = if args.demo {
        "synthetic".to_string()
    } else if args.input.is_empty() {
        "EGM2008".to_string()
    } else {
        args.input_datum.clone().context("local inputs require --input-datum (e.g. EGM2008, NAVD88, or unknown); heights must be in metres")?.trim().to_string()
    };
    ensure!(!datum.trim().is_empty(), "input datum cannot be empty");
    ensure!(
        datum.chars().all(|c| !c.is_control()),
        "input datum cannot contain control characters"
    );
    ensure!(
        !use_etopo || args.input.is_empty() || datum.eq_ignore_ascii_case("EGM2008"),
        "ETOPO can only be combined with local inputs explicitly in EGM2008; vertical datum conversion is not implemented"
    );
    ensure!(
        !datum.eq_ignore_ascii_case("unknown") || args.input.len() <= 1,
        "multiple unknown-datum inputs cannot be combined reliably"
    );
    let halo = 4. * args.radius / (size - 1) as f64;
    let products = if use_etopo {
        sources::etopo_products(&region, halo)
    } else {
        vec![]
    };
    if let Some(path) = &args.save_grid {
        ensure!(
            path.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("tif") || e.eq_ignore_ascii_case("tiff")),
            "saved grid must end in .tif or .tiff"
        );
    }
    if args.dry_run {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "request":region,"grid_size":size,"working_spacing_m":2.*args.radius/(size-1) as f64,
                "grid_values_bytes":size*size*8,"maximum_triangles":2*(size-1)*(size-1),
                "maximum_raw_polygons":args.max_polygons,"download_limit_bytes":args.max_download_mb*1024*1024,
                "style":settings.style,
                "elevation_filter":settings.elevation_filter,
                "boundary_tolerance_m":settings.boundary_tolerance_m,"vertical_datum":datum,
                "products":products,"local_inputs":args.input,"out":args.out,
                "scope":"NOAA ETOPO global provider and explicit local rasters; other agency discovery is not implemented in this release. Raster memory figures exclude GDAL and geometry overhead."
            }))?
        );
        return Ok(());
    }
    ensure!(
        args.out
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("svg")),
        "output must end in .svg"
    );
    ensure!(
        args.force || (!args.out.exists() && !args.out.with_extension("json").exists()),
        "output exists; use --force to replace it"
    );
    if let Some(path) = &args.save_grid {
        ensure!(
            args.force || !path.exists(),
            "saved grid exists; use --force to replace it"
        );
    }
    eprintln!(
        "Sampling {} km radius at {:.2} m spacing ({}² nodes)",
        args.radius / 1000.,
        2. * args.radius / (size - 1) as f64,
        size
    );
    let mut files = vec![];
    let mut cache = Cache::new(
        args.cache_dir,
        args.offline,
        args.max_download_mb * 1024 * 1024,
    )?;
    for (i, product) in products.iter().enumerate() {
        eprintln!("Source {}/{}: {}", i + 1, products.len(), product.id);
        files.push(cache.get(product)?);
    }
    for path in &args.input {
        let path = path
            .canonicalize()
            .with_context(|| format!("locating {}", path.display()))?;
        let (sha256, bytes) = sources::checksum(&path)?;
        files.push((
            path.clone(),
            Asset {
                product: Product {
                    id: path.file_name().unwrap().to_string_lossy().into(),
                    provider: "explicit local input".into(),
                    url: path.to_string_lossy().into(),
                    attribution:
                        "User-supplied raster; source attribution must be retained by its owner."
                            .into(),
                    vertical_datum: datum.clone(),
                    native_spacing_arcseconds: None,
                    surface_type: "user-declared elevation raster".into(),
                    published_vertical_accuracy: None,
                },
                sha256,
                bytes,
                contributing_grid_nodes: 0,
            },
        ));
    }
    if let Some(path) = &args.save_grid
        && let Ok(existing) = path.canonicalize()
    {
        ensure!(
            !files
                .iter()
                .any(|(source, _)| source.canonicalize().ok().as_ref() == Some(&existing)),
            "saved grid cannot replace an input or cached source raster"
        );
    }
    let (grid, assets) = if args.demo {
        (
            Grid::analytic(size, |x, y| {
                -5500.
                    + 9700. * (-((x - 0.15).powi(2) + (y + 0.05).powi(2)) * 18.).exp()
                    + 1800. * (-((x + 0.45).powi(2) + (y - 0.25).powi(2)) * 40.).exp()
            })?,
            vec![],
        )
    } else {
        raster::sample_files(&region, size, &files)?
    };
    eprintln!("Building and validating convex elevation bands...");
    let terrain = terrain::build(&grid, &settings)?;
    ensure!(
        terrain.accuracy.coverage_fraction >= args.min_coverage,
        "coverage {:.6} is below requested {:.6}",
        terrain.accuracy.coverage_fraction,
        args.min_coverage
    );
    let report = Report::new(region.clone(), datum, assets, &terrain);
    output::save(&args.out, &terrain, &report, args.width, args.force)?;
    if let Some(path) = args.save_grid {
        raster::save_grid_with_options(&path, &region, &grid, &report.vertical_datum, args.force)?;
    }
    eprintln!(
        "Wrote {} and {}: {} working convex pieces, {} unioned contour components, {:.3}% source coverage, {:.3}% retained circle area, SVG audit RMSE {:.3} m, quantization bound {:.3} m",
        args.out.display(),
        args.out.with_extension("json").display(),
        terrain.accuracy.polygon_count,
        terrain
            .layers
            .iter()
            .map(|l| l.footprint.len())
            .sum::<usize>(),
        100. * terrain.accuracy.coverage_fraction,
        100. * terrain.accuracy.retained_fraction,
        terrain.accuracy.audit_rmse_m.unwrap_or(0.),
        terrain.accuracy.quantization_bound_m
    );
    Ok(())
}
