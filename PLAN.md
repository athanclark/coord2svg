# coord2svg: proposed implementation plan

Status: implementation started. Export now Boolean-unions each elevation and paints cumulative contour footprints, following the request to remove shared-edge rendering artifacts. Convex pieces remain the working representation; final unioned paths can be concave. The first working version includes the convex geometry pipeline, Rust CLI, NOAA ETOPO global acquisition, local raster overlays, numeric reports, offline caching, and tests. Regional provider discovery, automatic vertical conversions, and bundled native desktop releases remain future milestones.

The intended product accepts a WGS84 latitude and longitude, a radius measured in metres, and optional detail controls. It produces a circular SVG of solid, unioned elevation contours built from convex pieces, grouped by elevation and painted from white at the lowest represented elevation to black at the highest. Bathymetry uses negative elevations and shares the same numeric scale as land.

## 1. Portable program and outputs

Use Rust for the command-line interface, processing orchestration, geometry, SVG generation, and validation. Use GDAL and PROJ for raster formats and coordinate/datum transformations. Keep these dependencies behind a raster/coordinate interface so geometry and SVG tests can run without remote services.

The release target is packaged applications for Windows, macOS, and Linux, with x86-64 and ARM64 builds where the native dependencies support them. Package the required GDAL/PROJ libraries and coordinate database with the application; users should not need to assemble a GIS installation. An optional container provides another reproducible distribution. Actual portability will be established by release smoke tests, rather than assumed from the language choice.

Primary outputs:

- `terrain.svg`: self-contained vector geometry with embedded metadata and no external assets.
- `terrain.json`: request parameters, source manifest, reference systems, sampling decisions, elevation/color table, coverage, and measured conversion errors.
- Optional intermediate GeoTIFF: the fused, projected elevation grid, for independent inspection.

Illustrative interface, subject to implementation:

```text
coord2svg --lat 21.3 --lon -157.8 --radius 20km --interval 25m --resolution 30m --out terrain.svg
```

Support automatic detail settings, explicit elevation intervals or level counts, local raster inputs, cached offline operation, fixed color ranges for comparisons, and a dry-run estimate of downloads, working memory, and output complexity. An SVG has vector resolution, but the underlying terrain still has finite source resolution.

## 2. Official sources and the meaning of coverage

There is no exhaustive worldwide download interface for every official elevation product. Implement a source registry with independent adapters and geographic coverage information. For each request, discover intersecting products from every enabled adapter, record the candidates, and select suitable products using documented rules. Report failed searches, inaccessible products, excluded sources, and uncovered areas. Do not claim that a run queried every agency worldwide.

Initial adapters:

| Source | Intended role | Important qualification |
| --- | --- | --- |
| NOAA ETOPO 2022 | Worldwide land and seabed fallback | 15 arc-second grid; select ice surface or bedrock explicitly. |
| GEBCO | Alternative global relief and seabed source | Pin the release, retain source-type information, and check published errata. Its approximate mean-sea-level reference requires care when combining datasets. |
| USGS 3DEP through The National Map | Finer bare-earth land terrain where covered | Discover actual products and their resolution, acquisition dates, datum, and accuracy metadata. |
| NOAA coastal DEMs/CUDEM | Finer coastal topography and bathymetry where covered | Tile-specific reference systems and source provenance matter. |
| Copernicus GLO-30/GLO-90 public products | Finer international land coverage | This is a surface model including vegetation and structures. Public coverage and access methods vary; identify it as a surface fallback. |

Extend the registry to other national mapping and hydrographic agencies in later increments. Also accept user-supplied official rasters with their provenance. Prefer suitable bare-earth terrain over a surface model for land, and suitable measured seabed products over coarse estimated bathymetry. Resolution alone does not establish accuracy. Avoid averaging overlapping products merely because they overlap.

Download spatial subsets or relevant raster blocks where the service supports them, with bounded downloads, retries, checksums, and a persistent cache. Maintain a small resampling halo if needed, but output only the requested circle. Freeze source identifiers, versions, URLs, and checksums in the manifest for repeatable runs. Preserve required attribution in the SVG and report.

## 3. Coordinates, elevation references, and sampling

Define the radius as shortest geodesic distance on the WGS84 ellipsoid. Use a local ellipsoidal azimuthal equidistant projection centered on the input coordinate, so radius is meaningful in metres. Handle longitude wrapping and polar centers explicitly. Convexity refers to the two-dimensional SVG coordinates. For very large radii, report projection distortion and enforce documented limits before the projection becomes ambiguous; do not silently switch to approximate degree-based distances.

Normalize supported elevations to signed metres in a declared vertical reference, preferably EGM2008 where defensible transformations exist. Do not silently equate ellipsoid heights, NAVD88, tidal datums, and mean-sea-level approximations. If a source cannot be harmonized reliably, exclude it from the composite and report the fallback, or use it alone with its declared reference and limitations. Zero denotes the chosen vertical reference, not the current ocean tide. A negative land elevation does not by itself imply ocean coverage; provider validity and land/water masks control source selection.

Build a projected elevation grid with an explicit sampling spacing and a documented interpolation method. Record native resolution and any resampling or downsampling. Use resource estimates to select an automatic detail setting, and expose the resulting choice. Missing values remain missing when supported fallbacks cannot fill them; show them as transparent coverage gaps rather than zero elevation. Preserve peaks and depressions within the chosen sampling model, and measure changes introduced by resampling.

## 4. Elevation levels and convex geometry

Implemented inclusive minimum/maximum elevation limits accept signed metres, kilometres, and international feet. Clip the continuous working terrain at those limits before calculating levels. Excluded areas remain transparent; report source coverage separately from retained area and audit exclusions. ETOPO download selection remains geographical. Automatic colors use the retained range; fixed color ranges remain independent.


Continuous terrain needs discrete display levels. The proposed default is approximately 64 levels, with an explicit interval option. Include the sampled minimum and maximum as display levels, and retain exact numeric elevations independently of color. The final defaults can be adjusted for the user's typical radius.

Use nearest-level quantization. Determine the minimum and maximum from valid terrain inside the requested circle, including interpolated boundary intersections. Let ordered display elevations be `h[0] ... h[n]`; adjacent classification boundaries are their midpoints. Each displayed region represents its group's elevation, and its exact numeric band is recorded. If the largest adjacent level spacing is `delta`, this stage contributes at most `delta / 2` metres of elevation error relative to the interpolated working terrain. Flat terrain produces one group and a declared single-level color convention.

Create a continuous piecewise-planar height model by triangulating the working grid with a deterministic diagonal convention. The working representation stores disjoint elevation bands. Each band is Boolean-unioned, then cumulative superlevel footprints are built and painted low to high. This preserves the visible stepped surface while dissolving internal mesh edges and preventing transparent antialiasing seams between elevation bands. Final paths are unions, so they can be concave and contain holes.

Clip every region to a convex polygon approximating the circular boundary. Select its segment count from a requested boundary tolerance, using the radial error bound `r * (1 - cos(pi / N))`. Features that intersect the boundary close along it; no terrain continuation outside the selected area is required.

Triangulate the clipped regions while respecting holes, then merge neighboring triangles whenever their union remains convex. As an implementation alternative, clipping individual height-model triangles against thresholds and the convex boundary directly also yields convex pieces. Internal edges can be dissolved before decomposition to reduce output complexity. Shared vertices and deterministic topology rules prevent geometric gaps.

The union of the final pieces should match the clipped elevation region within the declared numerical tolerance. A crescent-shaped ridge becomes multiple convex pieces, preserving its valley. The user confirmed this terrain-preserving convex decomposition as the intended geometry.

Geometry simplification is disabled by default until a tolerance can be enforced and measured. Any future simplification must revalidate convexity, coverage, nesting, and error limits. Working-piece convexity remains enforced; the explicitly requested final union may have concave outlines.

## 5. SVG structure and measurable color

Implemented configurable fill/stroke endpoints, sRGB and linear-light RGB interpolation, shortest/longest HSV hue arcs, rainbow, and repeatable custom stops. Repeatable per-elevation fill/stroke/width overrides insert in-range representative levels. Automatic scale endpoints follow the post-exclusion terrain extrema; explicit fixed ranges remain available. Stroke widths use non-scaling pixels and outline unioned superlevel boundaries. Report schema 3 records styles and actual per-level colors/widths.


Emit one `<g>` for each displayed elevation, in ascending numeric order. Each contains one Boolean-unioned compound superlevel path, preserving disconnected components and holes. Store representative elevation, classification boundary, band limits, and color as group attributes. Use solid opaque fills and no strokes by default.

For displayed elevation `h`, use equal sRGB channels:

```text
gray(h) = round(255 * (1 - (h - h_min) / (h_max - h_min)))
```

Here `h_min` and `h_max` are the declared color range; normally they are the local minimum and maximum display levels. Thus the endpoints are exactly `#ffffff` and `#000000`. The interpolation is linear in encoded sRGB channel values; it is not a claim of linear physical brightness. An optional linear-luminance mapping could be added later with its transfer function recorded.

An 8-bit grayscale has 256 channel values. More elevation groups can still retain their exact numeric identities, even when neighboring groups share a fill. Channel rounding contributes at most half a channel step; it does not alter geometry or the stored elevation. Export a table linking every group to its elevation band and fill. A fixed user-specified color range makes separate maps comparable, with clipping explicitly reported.

Embed center, radius, projection, elevation reference, terrain/surface mode, source attribution, detail settings, and conversion version in SVG metadata. Keep the main SVG artwork free of obligatory labels; optional legends or a separate legend file can expose the numeric scale.

Implemented global no-fill/no-stroke switches and repeatable every-N color/width patterns. Patterns count base levels after exclusion, without changing geometry or counting inserted specific levels; specific overrides and disable switches take precedence.

## 6. Accuracy report and validation

Separate errors introduced by this program from uncertainty already present in the source data. Do not sum RMSE, confidence percentiles, systematic datum offsets, and worst-case bounds as though they were interchangeable.

Report:

- Source coverage, nominal grid spacing, measurement/source types where available, acquisition dates, and published accuracy statistics with their original definitions. Unknown source accuracy remains unknown.
- Reference-system assumptions, transformations, exclusions, and any unresolved vertical uncertainty.
- Working-grid spacing, resampling residuals at audit locations, and maximum elevation-level spacing.
- Circle approximation error, coordinate rounding tolerance, and geometry area differences before and after convex decomposition.
- SVG-derived elevation versus the working terrain at deterministic interior audit points: bias, RMSE, maximum absolute error, sample count, and coverage. This measures conversion fidelity, not independently established Earth accuracy.
- Independent checkpoint residuals only where suitable independent, datum-compatible reference measurements are available. Label overlap comparisons as consistency checks rather than ground truth.
- Color range, mapping, quantization, monotonicity, duplicate colors, and polygon/file counts.

Core acceptance tests use analytic terrain: a plane, isolated hill, depression, saddle, disjoint islands, negative-depth terrain, mixed land/seabed, and flat terrain. Include features crossing the radius, internal holes, missing data, tile boundaries, longitude wrapping, polar centers, and incompatible datum metadata.

Check that every individual polygon is convex, groups are numerically ordered, cumulative regions are nested, endpoints have the specified colors, holes survive decomposition, and boundary vertices stay within tolerance. Compare decomposition area to the original clipped region and audit the stepped elevation field against the height model. Render sample SVGs to inspect shared-edge artifacts without enlarging polygons to hide errors. Provider fixtures make automated tests independent of network availability; small live data smoke tests verify adapters separately.

Cross-platform tests compare geometry, elevations, colors, and reports within explicit tolerances. Deterministic ordering and serialization support reproducibility; runtime timestamps belong in a manifest field that can be excluded from content comparisons.

## 7. Delivery order

1. Build the offline geometry pipeline and numeric report using local rasters and analytic fixtures. Establish convexity, boundary, elevation, and color guarantees first.
2. Add one global NOAA ETOPO adapter and caching to demonstrate the complete coordinate-to-SVG workflow for land, ocean, coasts, poles, and the date line within supported radius limits.
3. Add USGS and NOAA regional discovery and quality upgrades, including datum-aware selection and source manifests.
4. Add Copernicus public land products and a versioned GEBCO adapter, including source-type metadata and geographic errata checks.
5. Package and smoke-test the supported platforms, document accuracy and resource limits, and produce reproducible real-world examples. Extend national adapters incrementally.

The user confirmed convex pieces that preserve the terrain and specified radii of 1–1,000 km, with Hawaii as the first example. The implementation accepts that full radius range, with adjustable detail and explicit resource limits. A visual interface can use the same library later; it is not required for the portable first version.

## References checked during planning

- [NOAA ETOPO product documentation](https://www.ncei.noaa.gov/products/etopo-global-relief-model)
- [GEBCO 2026 grid and elevation reference](https://www.gebco.net/data-products-gridded-bathymetry-data/gebco2026-grid)
- [GEBCO published errata](https://www.gebco.net/data-products/gridded-bathymetry-data/data-set-errata)
- [USGS National Map API](https://www.usgs.gov/faqs/there-api-accessing-national-map-data)
- [USGS elevation products](https://www.usgs.gov/faqs/what-types-elevation-datasets-are-available-what-formats-do-they-come-and-where-can-i-download)
- [NOAA coastal elevation models](https://www.ncei.noaa.gov/products/coastal-elevation-models)
- [Copernicus DEM product description](https://dataspace.copernicus.eu/explore-data/data-collections/copernicus-contributing-missions/collections-description/COP-DEM)
- [Copernicus public raster distribution documentation](https://copernicus-dem-30m.s3.amazonaws.com/readme.html)
- [PROJ ellipsoidal geodesic calculations](https://proj.org/en/stable/geodesic.html)
- [PROJ azimuthal equidistant projection](https://proj.org/en/stable/operations/projections/aeqd.html)
- [GDAL GeoTIFF support](https://gdal.org/en/stable/drivers/raster/gtiff.html)
- [GDAL network raster access](https://gdal.org/en/stable/user/virtual_file_systems.html)
- [SVG painting and color specification](https://www.w3.org/TR/SVG11/painting.html)
