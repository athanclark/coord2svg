use crate::{
    sources::{Asset, Region},
    terrain::{Accuracy, Level, Terrain},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{BufWriter, Write},
    path::Path,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Report {
    pub schema_version: u32,
    pub program_version: String,
    pub request: Region,
    pub horizontal_projection: String,
    pub boundary_tangential_scale_max_sampled: f64,
    pub area_reference: String,
    pub vertical_datum: String,
    pub sources: Vec<Asset>,
    pub provider_scope: Vec<String>,
    pub warnings: Vec<String>,
    pub accuracy: Accuracy,
    pub levels: Vec<Level>,
    pub numeric_model: String,
    pub color_model: String,
    pub style: crate::style::Style,
    pub source_accuracy_measured: bool,
    pub resampling_error_measured: bool,
}
impl Report {
    pub fn new(region: Region, datum: String, sources: Vec<Asset>, terrain: &Terrain) -> Self {
        let mut warnings=vec!["Source accuracy has not been independently measured; nominal grid spacing is not an accuracy guarantee.".into(),
            "Bilinear raster resampling is followed by piecewise-planar terrain interpolation. Source-to-working-grid resampling error is not measured by the SVG audit.".into()];
        if terrain.accuracy.coverage_fraction < 0.999 {
            warnings
                .push("Incomplete valid terrain coverage; missing regions are transparent.".into());
        }
        if sources
            .iter()
            .any(|a| a.product.native_spacing_arcseconds == Some(15.))
        {
            warnings.push("ETOPO's 15 arc-second spacing is approximately 460 m north-south; small islands and features below the working/source resolution may be absent.".into());
        }
        if datum.eq_ignore_ascii_case("unknown") {
            warnings.push("Vertical reference is unknown; these elevations cannot be treated as established sea-level heights.".into());
        }
        let min = terrain.accuracy.minimum_elevation_m;
        let max = terrain.accuracy.maximum_elevation_m;
        let mut unused: Vec<_> = terrain
            .style
            .elevations()
            .filter(|z| *z < min || *z > max)
            .collect();
        unused.sort_by(f64::total_cmp);
        unused.dedup();
        if !unused.is_empty() {
            warnings.push(format!("Specific style elevations outside the retained terrain range were ignored (metres): {unused:?}"));
        }
        Self{schema_version:3,program_version:env!("CARGO_PKG_VERSION").into(),horizontal_projection:region.proj4(),
            boundary_tangential_scale_max_sampled:region.sampled_tangential_scale(),area_reference:"Areas are in the local azimuthal equidistant plane, not ellipsoidal surface areas. Boundary tangential scale is sampled at 360 azimuths; center-to-point radial distances are preserved.".into(),request:region,
            vertical_datum:datum,sources,provider_scope:vec!["NOAA ETOPO 2022 v1 (global, ice-surface version)".into(),"explicit local single-band rasters".into()],
            warnings,accuracy:terrain.accuracy.clone(),levels:terrain.layers.iter().map(|l|l.level.clone()).collect(),
            numeric_model:"Nearest representative elevation; band boundaries are midpoint heights. Visible elevation bands use nearest-height assignment. Unioned superlevel footprints are painted ascending, with lower fills extending underneath higher fills to avoid transparent antialiasing seams. Convex working pieces are Boolean-unioned per elevation using NonZero fill on the original i64 coordinate grid, preserving holes. One compound SVG path per superlevel footprint removes internal mesh edges; unioned outlines can be concave. Convexity uses exact i128 supporting-line predicates on normalized coordinates quantized to 1e-10 radius units. Only convex clipping primitives are canonicalized after rounding; concave terrain regions and merge candidates are never replaced with hulls. Inclusive elevation filters clip the working mesh before band quantization; excluded areas are transparent. No smoothing.".into(),
            style:terrain.style.clone(),
            color_model:"Opaque #RRGGBB colors; rounded 8-bit sRGB channels. Gradient positions span the retained elevation endpoints unless color_range explicitly fixes a scale. Modes: encoded sRGB, linear-light sRGB, HSV shortest/longest hue path, or rainbow stops. Flat terrain uses the lowest color. Every-N patterns count base levels established by levels/interval after exclusion, anchored at the first base level at or above their start; inserted specific levels do not change pattern counting. Last matching pattern wins per attribute, then specific overrides, then no-fill/no-stroke switches. Stroke widths use non-scaling screen pixels and outline unioned elevation boundaries. Legacy gray is the reference grayscale channel, not custom-color luminance; fill is authoritative.".into(),
            source_accuracy_measured:false,resampling_error_measured:false}
    }
}
pub fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
pub fn write_svg(
    mut writer: impl Write,
    terrain: &Terrain,
    report: &Report,
    width: u32,
) -> Result<()> {
    writeln!(writer, "<?xml version=\"1.0\" encoding=\"UTF-8\"?>")?;
    writeln!(
        writer,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"-1 -1 2 2\" width=\"{width}\" height=\"{width}\" color-interpolation=\"sRGB\">"
    )?;
    writeln!(
        writer,
        "<title>Terrain at {}, {}, radius {} m</title>",
        report.request.latitude, report.request.longitude, report.request.radius_m
    )?;
    writeln!(
        writer,
        "<desc>Unioned elevation bands with configurable elevation-based colors and strokes. Heights in metres, vertical reference {}. Missing data is transparent.</desc>",
        xml_escape(&report.vertical_datum)
    )?;
    writeln!(
        writer,
        "<metadata id=\"coord2svg-report\">{}</metadata>",
        xml_escape(&serde_json::to_string(report)?)
    )?;
    // Exact visual circle also protects against sub-millimetre coordinate snapping.
    writeln!(
        writer,
        "<defs><clipPath id=\"circle\"><circle cx=\"0\" cy=\"0\" r=\"1\"/></clipPath></defs>"
    )?;
    writeln!(writer, "<g clip-path=\"url(#circle)\" stroke=\"none\">")?;
    for (i, layer) in terrain.layers.iter().enumerate() {
        let l = &layer.level;
        writeln!(
            writer,
            "<g id=\"elevation-{i}\" data-elevation-m=\"{}\" data-band-min-m=\"{}\" data-band-max-m=\"{}\" fill=\"{}\" stroke=\"{}\" stroke-width=\"{}\" stroke-linejoin=\"round\">",
            l.elevation_m,
            l.band_min_m,
            l.band_max_m,
            l.fill,
            if l.stroke_width_px > 0. {
                l.stroke_fill.as_str()
            } else {
                "none"
            },
            l.stroke_width_px
        )?;
        // One paint operation for the entire superlevel footprint; holes have
        // opposite winding and lower fills cover shared band joins underneath.
        write!(
            writer,
            "<path id=\"stratum-{i}\" fill-rule=\"nonzero\" vector-effect=\"non-scaling-stroke\" d=\""
        )?;
        for shape in &layer.footprint {
            for ring in &shape.rings {
                for (j, v) in ring.iter().enumerate() {
                    write!(
                        writer,
                        "{} {:.10},{:.10} ",
                        if j == 0 { "M" } else { "L" },
                        v.x,
                        -v.y
                    )?;
                }
                write!(writer, "Z ")?;
            }
        }
        writeln!(writer, "\"/>")?;
        writeln!(writer, "</g>")?;
    }
    writeln!(writer, "</g></svg>")?;
    Ok(())
}
/// Stage both files completely before replacing any destination. No-clobber is
/// the default. Each individual commit is atomic on its destination filesystem.
pub fn save(
    path: &Path,
    terrain: &Terrain,
    report: &Report,
    width: u32,
    force: bool,
) -> Result<()> {
    ensure!(
        path.extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("svg")),
        "output must end in .svg"
    );
    let json = path.with_extension("json");
    ensure!(
        force || (!path.exists() && !json.exists()),
        "output exists; use --force to replace the SVG/report pair"
    );
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let mut svg_temp = tempfile::NamedTempFile::new_in(parent)?;
    let mut json_temp = tempfile::NamedTempFile::new_in(parent)?;
    {
        let mut writer = BufWriter::new(svg_temp.as_file_mut());
        write_svg(&mut writer, terrain, report, width)?;
        writer.flush()?;
    }
    serde_json::to_writer_pretty(json_temp.as_file_mut(), report)?;
    svg_temp.as_file().sync_all()?;
    json_temp.as_file().sync_all()?;
    if force {
        json_temp.persist(&json).context("committing JSON report")?;
        svg_temp.persist(path).context("committing SVG")?;
    } else {
        json_temp
            .persist_noclobber(&json)
            .context("committing JSON report")?;
        svg_temp.persist_noclobber(path).context("committing SVG")?;
    }
    Ok(())
}
