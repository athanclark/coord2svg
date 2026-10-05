# Testing coord2svg

The suite checks numeric terrain conversion, native raster handling, CLI behavior,
SVG structure, and actual rendered pixels. Tests use analytic terrain, generated
GeoTIFFs, temporary directories, and local HTTP servers. They do not download
NOAA data or use files in the working cache.

## Run locally

Install the Rust and GDAL prerequisites from [README.md](README.md#build), then run:

```sh
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
cargo build --release --locked
```

To check one area, use an integration-test target, for example:

```sh
cargo test --locked --test end_to_end
cargo test --locked --test rendering
cargo test --locked --test union
```

There is no need to install Inkscape, a browser, or a PNG rendering utility. Pixel
tests use the pure Rust `resvg` development dependency on every platform.

## Coverage and acceptance criteria

| Test target | What a failure means |
| --- | --- |
| `geometry` | Convex working pieces, circle clipping, holes, nodata, deterministic output, or elevation-error bounds changed. Analytic and randomized fixtures cover hills, saddles, planes, depressions, and seabed. |
| `union` | Boolean union lost coverage, filled a hole, retained internal boundaries, mishandled overlapping pieces/T-junctions, or changed the represented elevation field. |
| `rendering` | Stroke-free interiors contain unwanted transparent seams, or excluded terrain/holes became visible. These are pixel assertions, not screenshot snapshots. |
| `elevation_filter` | Physical thresholds, inclusive bounds, units, source-versus-selected coverage, or color endpoints after exclusion are incorrect. |
| `style` | Known interpolation values, gradient stops, endpoint colors, hue modes, exact-height overrides, or exported style metadata are incorrect. |
| `patterns` | Every-N patterns shifted when specific levels were inserted, ignored precedence, changed geometry, or failed to honor paint-disabling switches. |
| `raster` | Real GDAL reprojection, vertical-datum declarations, units, scale/offset, nodata, overlays, Unicode paths, or date-line handling regressed. |
| `sources` | Source selection, geodesic bounds, caching, checksums, retry behavior, or download limits regressed. HTTP fixtures have bounded connection/read/write waits. |
| `output_cli` | Output protection, serialized SVG geometry, metadata escaping, determinism, or CLI validation regressed. |
| `end_to_end` | The public CLI no longer converts a local GeoTIFF into consistent SVG/JSON/rendered output, fails to preserve unfiltered saved grids, writes outputs for an empty selection, or loses pattern/override semantics. |

Elevation audits compare the final SVG geometry with the working terrain mesh;
they do not establish real-world source accuracy. Geometry assertions use
explicit numeric tolerances where resampling or coordinate quantization can
introduce roundoff. Style metadata uses exact JSON floating-point round trips,
so equivalent SVG and report elevations can be matched without fuzzy identity.

The randomized tests retain regression cases when they find a failure. Keep the
reported seed/case and add a focused fixture when fixing a new defect. Do not
weaken tolerances or remove hole/opacity assertions to get a green build.

## GitHub Actions

[The CI workflow](.github/workflows/ci.yml) runs on pushes, pull requests, and
manual dispatch. It performs:

- The entire suite on current stable Rust on Linux, macOS, and Windows.
- The entire suite on Linux with the declared minimum Rust version, 1.88.0.
- Formatting and Clippy checks with warnings treated as errors on stable Linux.
- A release build and offline synthetic CLI smoke run on each stable platform,
  including filters, styles, exact-height overrides, and saved GeoTIFF output.
- Artifact uploads for smoke SVG/JSON/GeoTIFF files and unbundled executables;
  property-test regression files are uploaded on failure when present.

Each runner installs its GDAL dependency. Rust caches include the OS, compiler,
and GDAL version to avoid sharing builds across incompatible native setups.
Jobs have time limits and newer runs cancel superseded runs on the same ref.

Artifacts contain unbundled executables: GDAL/PROJ must still be installed on the
machine where they are used. CI jobs for macOS, Windows, and minimum Rust must
actually pass on GitHub before treating those configurations as verified. Local
workflow linting and Linux results do not establish remote runner results.
