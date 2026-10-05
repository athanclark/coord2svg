# coord2svg

Convert an Earth coordinate and a **1–1,000 km radius** into a self-contained SVG of unioned elevation contours derived from convex terrain pieces. Land and seabed share a signed metre scale. By default, white is the lowest displayed elevation and black is the highest; custom fill/stroke gradients and per-elevation styles are supported. Polygons are grouped by elevation and written in ascending order.

The first release automatically downloads **NOAA ETOPO 2022 v1**, a global land/bathymetry/ice-surface model, and supports explicit local raster inputs and compatible overlays. Automatic USGS, CUDEM, Copernicus, and GEBCO discovery and vertical datum transformations are **not implemented yet**. It does not claim to discover every official dataset worldwide. The original design and delivery milestones are in [PLAN.md](PLAN.md).

## Build

Requires Rust 1.88 or newer, a C compiler, and GDAL development/runtime files (GDAL 3.4 or newer, with PROJ). The small C interface compiles against installed GDAL headers; no generated Rust GDAL bindings or libclang are required.

Ubuntu/Debian:

```sh
sudo apt-get install build-essential pkg-config libgdal-dev
cargo build --release --locked
```

macOS:

```sh
brew install gdal pkg-config
cargo build --release --locked
```

Windows: install the Rust MSVC toolchain and Visual Studio C++ build tools. One GDAL option is a conda-forge environment with `libgdal-core`. In an activated PowerShell environment, set `GDAL_INCLUDE_DIR` to the directory containing `gdal.h` and `GDAL_LIB_DIR` to the directory containing `gdal.lib`. Add GDAL's DLL directory to `PATH`. The CI workflow configures this automatically for Windows tests.

The executable is `target/release/coord2svg` (`coord2svg.exe` on Windows). It currently needs the native GDAL/PROJ installation at runtime. Cross-platform CI is configured; local verification has been on Linux. Standalone bundled desktop releases remain a later milestone.

## Hawaii: first run

This circle includes the main Hawaiian islands and their surrounding seabed:

```sh
cargo run --release --locked -- \
  --lat 20.9 --lon -157.4 --radius 500km \
  --resolution 2km --levels 64 \
  --out output/hawaii.svg --save-grid output/hawaii.tif
```

Outputs: `hawaii.svg`, `hawaii.json`, and optionally `hawaii.tif`. A real NOAA example was generated at these settings during development. **This example covers the main islands, not the entire archipelago extending through the Northwestern Hawaiian Islands.** Use additional circles to explore that longer chain within the supported radius range.

Preview with a browser, Inkscape, or another SVG viewer. For a PNG with librsvg:

```sh
rsvg-convert output/hawaii.svg -o output/hawaii.png
```

Distances accept `m` or `km`; bare numbers are metres. Latitude/longitude are WGS84 decimal degrees. Zero elevation is EGM2008 for the ETOPO provider, not the instantaneous tide.

## Elevation limits

Use either inclusive height limit, or both:

```sh
# Main Hawaiian islands above the EGM2008 zero-height surface.
cargo run --release --locked -- \
  --lat 20.9 --lon -157.4 --radius 500km --resolution 2km \
  --min-elevation 0m --offline --out output/hawaii-land.svg

# Keep only heights from zero through 10,000 international feet.
cargo run --release --locked -- \
  --lat 20.9 --lon -157.4 --radius 500km --resolution 2km \
  --min-elevation 0 --max-elevation 10000ft \
  --offline --out output/hawaii-land-10000ft.svg
```

Elevation limits accept signed metres (bare numbers or `m`), `km`, `mi`, or `ft`, including quoted values such as `"10000 ft"`. One [international foot is exactly 0.3048 metres](https://www.nist.gov/pml/us-surveyfoot), so `10000ft` becomes `3048m`. Negative heights work, e.g. `--min-elevation=-5000m`. Limits refer to the input's declared vertical datum; a zero-height selection is not a land/water classification.

The program clips the continuous working mesh at these exact heights **before** calculating elevation bands. Excluded regions are transparent, including holes within retained terrain. Limits are inclusive; equal limits retain flat regions exactly at that height. An entirely excluded or zero-area selection fails with an explanatory error. Automatic white-to-black colors and level spacing use the retained elevation range; `--color-range` remains an independent fixed scale.

ETOPO tiles are selected geographically, so elevation filtering does **not** reduce current downloads or raster sampling. It reduces subsequent geometry work and output size. `--dry-run` reports limits normalized to metres. `--save-grid` saves the full unfiltered working raster for reuse.

The report distinguishes `coverage_fraction` / `covered_area_m2` (valid source terrain before filtering) from `retained_fraction` / `retained_area_m2` (selected terrain). Both fractions use the ideal projected circle as denominator. `--min-coverage` checks source coverage independently of the filter. `elevation_filter` records the bounds, and `audit_elevation_excluded` counts intentionally excluded audit samples separately from nodata and missing geometry.

## Fill gradients and strokes

Automatic scaling always spans the **actual retained minimum and maximum after exclusion**, rather than the original terrain or the requested limits when those lie beyond available terrain. An explicit `--color-range=MIN:MAX` intentionally fixes a different metre scale across maps. Both fill and stroke gradients use that same elevation scale. Specific overrides take precedence, including at the endpoints. Flat terrain uses the lowest color.

```sh
cargo run --release --locked -- \
  --lat 20.9 --lon -157.4 --radius 500km --resolution 2km \
  --min-elevation 0 --max-elevation 10000ft \
  --lowest-fill '#fff4cc' --highest-fill '#243b70' \
  --fill-gradient linear-rgb --fill-stop '0.5,#3f9b70' \
  --lowest-stroke-fill '#e2c980' --highest-stroke-fill '#101830' \
  --stroke-gradient srgb --stroke-width 0.4px \
  --specific-fill '0ft,#ffffff' --specific-fill '1mi,#d65a31' \
  --specific-stroke-fill '0ft,#00aaff' \
  --specific-stroke-width '0ft,1px' \
  --out output/hawaii-styled.svg
```

Quote colors and comma-separated settings so shell `#` comments cannot consume them. Colors are opaque six-digit `#RRGGBB`; input hex case is normalized. The matching fill/stroke options are:

| Fill | Stroke | Meaning |
| --- | --- | --- |
| `--lowest-fill` | `--lowest-stroke-fill` | Color at lowest retained elevation |
| `--highest-fill` | `--highest-stroke-fill` | Color at highest retained elevation |
| `--fill-gradient` | `--stroke-gradient` | Interpolation mode |
| `--fill-stop POSITION,#RRGGBB` | `--stroke-stop POSITION,#RRGGBB` | Repeatable intermediate color stop |
| `--specific-fill HEIGHT,#RRGGBB` | `--specific-stroke-fill HEIGHT,#RRGGBB` | Repeatable representative-elevation override |

Modes are `srgb` (default, encoded-channel interpolation), `linear-rgb` (decode sRGB, interpolate light, encode), `hsv-short`, `hsv-long`, and `rainbow`. HSV modes use the shorter/longer hue arc between saturated colors; achromatic endpoints borrow the other endpoint's hue. A half-turn tie uses the decreasing arc for `hsv-short`; identical saturated hues make a full turn for `hsv-long`. Rainbow passes through saturated red, yellow, green, cyan, blue, and magenta at positions 1/7 through 6/7, while retaining the requested endpoint colors. Custom stops have unique positions strictly between 0 and 1, are sorted automatically, and interpolate using the selected mode. Rainbow supplies its own stops and cannot be combined with explicit stops. Every resulting channel is rounded to the nearest 8-bit sRGB integer; stop and endpoint colors are exact.

`--stroke-fill '#123456'` gives every elevation a single stroke color and conflicts with stroke endpoint/gradient/stop flags. `--stroke-width 2px` sets the common width; bare widths also mean pixels. Strokes default off, or to 1px when a global stroke color/gradient is supplied. A lone `--specific-stroke-fill` enables a 1px stroke only at its elevation. Explicit global widths, including 0px, take precedence over that automatic default; `--specific-stroke-width HEIGHT,WIDTHpx` then overrides individual widths. Widths must be finite and nonnegative.

Specific heights accept signed `m`, `km`, `ft`, or international `mi` (1mi = 1609.344m). An override inserts that exact height into the representative level set when it is in the retained range, rather than silently choosing a nearby level. It applies to that level's nearest-height **band**, not an infinitesimal contour line. Insertion adjusts adjacent band boundaries and updates numeric accuracy metrics. The total remains capped at 1,024 levels. Duplicate overrides of the same attribute at equivalent heights are rejected. Heights excluded by the filter or outside the retained range are ignored and listed in report warnings; they never restore excluded terrain.

Strokes outline **unioned superlevel boundaries**, including holes and any selected-radius or elevation-exclusion boundary. Internal convex decomposition edges are dissolved. Stroke widths use [SVG non-scaling strokes](https://www.w3.org/TR/SVG/painting.html#PaintingVectorEffects), independent of the normalized map coordinates. Strokes can visually cover neighboring bands; geometry audits still measure the underlying filled polygons, without their painted stroke footprints. The [sRGB transfer function](https://www.w3.org/TR/css-color-4/#color-conversion-code) defines linear-light conversion.

JSON/SVG metadata uses report schema version 3 and includes normalized style settings plus each level's authoritative `fill`, `stroke_fill`, and `stroke_width_px`. The legacy `gray` value remains a reference grayscale channel; it is not the luminance of a custom fill.

## Disable paint and repeat styles by level

`--no-fill` emits `fill="none"` on every elevation group. `--no-stroke` emits `stroke="none"` with zero width. Both switches take precedence over gradients, patterns, and specific overrides. They affect painting only: geometry and accuracy reports still describe the retained terrain. Both switches together produce transparent artwork. `--no-fill` alone does not enable strokes; supply a stroke color, width, or pattern to make outlines visible.

Repeatable patterns have the form `START_HEIGHT,N,STYLE`:

```sh
cargo run --release --locked -- \
  --lat 20.9 --lon -157.4 --radius 500km --resolution 2km \
  --min-elevation 0 --levels 64 --no-fill \
  --stroke-fill '#333333' --stroke-width 0.2px \
  --every-n-stroke-fill '0ft,5,#000000' \
  --every-n-stroke-width '0ft,5,1px' \
  --out output/hawaii-patterns.svg

# Other repeatable options (append to a generation command):
# --every-n-fill '0ft,5,#ff6600'
# --every-n-stroke-fill '0ft,5,#000000'
# --every-n-stroke-width '0ft,5,10px'
```

Patterns count the **base levels already established by `--levels` or `--interval`, after elevation exclusion**. They do not insert elevations or change geometry. The anchor is the first base level at or above `START_HEIGHT`; it is styled, followed by levels at offsets N, 2N, and so on. Thus N=5 means base indices 0, 5, 10, ... relative to the anchor, and N=1 applies to every subsequent base level. A start between levels advances to the next higher one; a start below the retained range anchors at its first base level; a start above it matches nothing. A flat terrain has one base level.

Specific overrides may insert extra elevation groups; these do not count toward N and do not receive pattern styles. Metadata exposes each group's `base_level_index` (zero-based, or null for an inserted group). Pattern starts accept the same signed height units as specific overrides. N must be a positive integer, and width patterns accept finite nonnegative pixels.

Precedence per attribute is gradient/global style, then patterns in argument order (last matching pattern wins), then specific overrides, then the disabling switches. A lone `--every-n-stroke-fill` supplies 1px strokes only on matching levels. Explicit global widths disable this automatic default, and explicit width patterns override automatic pattern widths.

These strokes follow unioned superlevel outlines. The example gives periodic elevation emphasis without drawing the internal convex decomposition edges.

## Detail and resource controls

The default is 513 sample nodes per axis, 64 display levels, and 4,096 deterministic audit locations. Detail is independent of radius: the default working spacing is `diameter / 512`. At 1,000 km radius that is approximately 3.91 km. Use `--resolution` or `--grid-size` to choose finer detail; requests exceeding 2,049 nodes per axis fail before downloading. Sampling finer than a source grid cannot create additional surveyed detail.

```sh
# Print the source and resource plan without downloading or writing files.
cargo run --release -- --lat 24 --lon -167 --radius 1000km --dry-run

# Use an explicit elevation interval, independent of the working grid.
cargo run --release -- --lat 20.9 --lon -157.4 --radius 500km \
  --resolution 2km --interval 100m --out output/hawaii-100m.svg

# Reuse validated downloads without network access; protect existing outputs.
cargo run --release -- --lat 20.9 --lon -157.4 --radius 500km \
  --offline --out output/hawaii-offline.svg
```

`--levels` and `--interval` are alternatives; levels are capped at 1,024. An interval adds the minimum and maximum as endpoint levels, so the final gap may be smaller. `--boundary-tolerance` controls the circle's polygon approximation in metres. `--color-range=-8000:4500` fixes a color range across maps; out-of-range values clamp to white/black. Flat terrain uses white.

The defaults cap new download payloads at 512 MiB and raw geometry at 2,000,000 pieces. Override with `--max-download-mb` and `--max-polygons`. Each source download is limited to 128 MiB; cache hits do not consume the download budget. Geometry, audit indexing, and native raster operations use additional memory beyond the grid-value estimate printed by `--dry-run`. A large high-detail SVG can contain hundreds of thousands of polygons and be tens of megabytes. Budget errors recommend reducing sampling detail or elevation levels.

Cache files live in `.cache/coord2svg` by default; `--cache-dir` chooses another location. Source URLs key the cache. SHA-256 and byte lengths are checked on every hit. Interrupted/invalid downloads are not committed. The initial adapter fetches intersecting 15-degree GeoTIFF tiles, not block-level spatial subsets.

Existing SVG/report pairs are protected unless `--force` is supplied. Files are staged before committing; each individual destination is atomic, but the SVG/report pair is not a filesystem-wide transaction. `--min-coverage 0.99` can require a minimum valid area fraction; otherwise gaps remain transparent and are reported.

## Local data and overlays

Use a single-band georeferenced raster in metres, with its reference system and missing-data mask/nodata correctly specified. GDAL-supported formats such as GeoTIFF and suitably georeferenced NetCDF are accepted. UTF-8 paths, including spaces and Unicode, are supported.

```sh
# Passing local inputs automatically selects local-only mode.
cargo run --release -- --lat 21.3 --lon -157.8 --radius 10km \
  --input official-dem.tif --input-datum EGM2008 \
  --resolution 30m --out output/local.svg

# Add a compatible local high-resolution tile over global ETOPO.
cargo run --release -- --lat 21.3 --lon -157.8 --radius 10km \
  --source etopo --input official-egm2008-dem.tif --input-datum EGM2008 \
  --out output/overlay.svg
```

Repeat `--input` for compatible sources in increasing priority order. Later valid pixels override earlier sources; nodata preserves a valid fallback. Reports count final winning grid nodes, including nodes outside the circular artwork in the working square. Scale and offset metadata are applied. Declared elevation units other than metres are rejected.

`--input-datum` is a declaration, **not a conversion**. Embedded `VERTICAL_REFERENCE` metadata must agree when present. ETOPO overlays require an explicit EGM2008 declaration. Single local inputs can use another declared reference, such as NAVD88 or MSL; the output retains that label. Unknown-reference inputs must run alone. Full compound-CRS vertical authority validation and transformations remain future work; declaring a datum does not repair incorrect source metadata. Keep the original data source's required attribution alongside outputs from local inputs.

## Geometry, colors, and accuracy

The working grid becomes a continuous mesh with a fixed northwest-to-southeast diagonal. Each triangle is clipped to the circular polygon and elevation band. Convex clipping preserves valleys, channels, and depressions. Coordinates are rounded to `1e-10` radius units, and individual convex clipping primitives are canonicalized after rounding. Neighboring pieces merge only when their union is a simple convex ring; exact integer supporting-line and distinct-vertex checks reject self-touching loops, stars, and concavity at exported coordinate precision. Concave terrain regions are never replaced by convex hulls, and no smoothing is applied.

Working pieces in each band undergo a true Boolean union on the existing i64 coordinate grid, with NonZero filling so overlaps add coverage rather than creating holes. The SVG stores **one compound path per elevation group**, with disconnected regions and holes as closed subpaths. Unioned outlines can be concave; convexity is retained in the working pieces used to build them, rather than required of the final unioned path. This follows the request to union each stratum like Inkscape.

For painting, each level uses its unioned **superlevel footprint**: its visible band plus every higher band. Lower fills extend underneath higher ones, in ascending elevation order. Separately painting adjacent pieces or disjoint bands can leave antialiasing seams even with `stroke="none"`; dissolving internal edges and layering the footprints prevents those seams from revealing transparent background. Visible elevations, per-level colors, and override semantics remain the same. All footprints stay within the selected valid terrain; missing data and elevation-exclusion holes remain transparent. No strokes, shape dilation, or terrain smoothing are added to hide seams.

Each group records its representative elevation and visible band limits. Pieces that cross the radius close against the boundary. Polygon approximation is slightly inside the exact circle, with the measured radial error reported. A circular SVG clip also contains coordinate-rounding effects. `area_m2` and `union_component_count` / `union_ring_count` describe the visible band's union; `footprint_component_count` / `footprint_ring_count` describe the actual cumulative SVG path. `polygon_count` remains the number of working convex pieces.

Display elevations include the working terrain's in-circle minimum and maximum, including boundary intersections. Intermediate values use nearest-level assignment; ties select the higher level. For maximum neighboring level spacing Δ, quantization contributes at most Δ/2 metres relative to the unsnapped working mesh. Coordinate rounding is a separate spatial error; it can affect classifications extremely close to an elevation boundary.

For displayed height `h`, equal sRGB channels use `round(255 × (1 − clamp((h−min)/(max−min), 0, 1)))`. This is linear in encoded sRGB channel values, not physical luminance. There are 256 grayscale channel values; finer elevation groups retain separate numeric identities even if their fills coincide. Antialiasing and display/printer color management can change observed boundary pixels without changing the stored fills.

The JSON report is also embedded in SVG metadata and includes:

- Source identifiers, URLs, checksums, attribution, vertical reference, and nominal source spacing.
- Working spacing, projected covered area, coverage fraction, circle approximation and coordinate rounding bounds.
- Elevation bands, colors, working piece counts, union/footprint component and ring counts, and area changes from convex merging and Boolean unions. The union uses an explicit coordinate grid of `radius / 1e10` metres; cumulative footprint area differences are reported separately.
- Maximum sampled boundary tangential scale at 360 azimuths. The azimuthal equidistant projection preserves distances from the center; reported areas are projected areas, not ellipsoidal surface areas.
- Bias, RMSE, and maximum absolute residual of the **final unioned superlevel SVG geometry** versus the working mesh at deterministic equal-area disk samples. Point containment uses a separately reported positional tolerance of `1e-10` radius units. Boundary-approximation exclusions, source nodata, and elevation-filter exclusions are counted separately. Missing valid audit points abort export.

These residuals measure conversion fidelity. They **do not establish source accuracy, resampling error, or independent Earth accuracy**. Those are explicitly marked unmeasured. ETOPO's nominal 15 arc-second grid is about 460 m north-south, and its bathymetry includes interpolated data. Small atolls or summits can be missed when either the source grid or working grid is too coarse. Published uncertainty is retained when supplied; unknown uncertainty is not invented.

Official source documentation: [NOAA ETOPO](https://www.ncei.noaa.gov/products/etopo-global-relief-model), [ETOPO metadata](https://www.ncei.noaa.gov/access/metadata/landing-page/bin/iso?id=gov.noaa.ngdc.mgg.dem:etopo_2022), [PROJ azimuthal equidistant projection](https://proj.org/en/stable/operations/projections/aeqd.html).

## Tests and development

```sh
cargo test --locked
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
```

The offline suite covers analytic planes, hills, saddles, depressions, disjoint islands, seabed, flat terrain, missing data, boundary crossings, 1–1,000 km scaling, randomized clipping/height planes, self-touching merge regressions, geodesic round trips, polar/date-line discovery, actual GeoTIFF reprojection across the date line, datum/unit rejection, scale/offset, overlays, cache corruption and HTTP failure budgets, serialized SVG geometry, Boolean unions with overlaps and holes, portable raster checks for interior opacity and elevation-exclusion holes, numeric color mapping, output protection, and CLI validation. Tests use temporary local files and loopback HTTP fixtures; no NOAA connection is required.

The GitHub Actions workflow configures Linux, macOS, and Windows builds/tests and release executable artifacts. It has not been run remotely from this workspace. A synthetic smoke test is available without official data:

```sh
cargo run --release -- --lat 0 --lon 0 --radius 1km --demo --out output/demo.svg
```

## Container

The container recipe includes the required GDAL/PROJ runtime. Its build could not be verified in this workspace because access to the Docker daemon socket was denied:

```sh
docker build -t coord2svg .
docker run --rm -v "$PWD:/work" coord2svg \
  --lat 20.9 --lon -157.4 --radius 500km --out output/hawaii.svg
```

On Linux, adding `--user "$(id -u):$(id -g)"` keeps output files owned by your account. On Windows, adapt the bind mount syntax for your shell. The repository's `.dockerignore` excludes local rasters, downloads, and generated output from the build context.
