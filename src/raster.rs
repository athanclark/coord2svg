//! GDAL boundary. Buffers are caller-owned; dataset handles stay in C.
use crate::{
    sources::{Asset, Region},
    terrain::Grid,
};
use anyhow::{Context, Result, bail, ensure};
use std::{
    ffi::{CString, c_char, c_int},
    path::Path,
    sync::Once,
};
unsafe extern "C" {
    fn c2s_register();
    fn c2s_validate(path: *const c_char, error: *mut c_char, length: usize) -> c_int;
    fn c2s_warp(
        path: *const c_char,
        projection: *const c_char,
        declared_datum: *const c_char,
        size: c_int,
        radius: f64,
        values: *mut f64,
        error: *mut c_char,
        length: usize,
    ) -> c_int;
    fn c2s_write(
        path: *const c_char,
        projection: *const c_char,
        width: c_int,
        height: c_int,
        affine: *const f64,
        values: *const f64,
        nodata: f64,
        scale: f64,
        offset: f64,
        datum: *const c_char,
        unit: *const c_char,
        error: *mut c_char,
        length: usize,
    ) -> c_int;
}
fn register() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        // SAFETY: registration occurs once, before any other native call.
        unsafe { c2s_register() };
    });
}
fn filename(path: &Path) -> Result<CString> {
    CString::new(path.to_str().context("raster paths must be UTF-8")?)
        .context("raster path contains a NUL byte")
}
fn status(ok: c_int, error: &[c_char]) -> Result<()> {
    if ok == 0 {
        let bytes: Vec<u8> = error
            .iter()
            .take_while(|&&b| b != 0)
            .map(|&b| b as u8)
            .collect();
        bail!("{}", String::from_utf8_lossy(&bytes));
    }
    Ok(())
}
pub fn validate(path: &Path) -> Result<()> {
    register();
    let path = filename(path)?;
    let mut error = [0; 2048];
    // SAFETY: NUL-terminated path and correctly sized writable error buffer.
    let ok = unsafe { c2s_validate(path.as_ptr(), error.as_mut_ptr(), error.len()) };
    status(ok, &error)
}
pub fn sample_files(
    region: &Region,
    size: usize,
    files: &[(std::path::PathBuf, Asset)],
) -> Result<(Grid, Vec<Asset>)> {
    ensure!((3..=2049).contains(&size), "grid size must be 3..2049");
    region.validate()?;
    register();
    let projection = CString::new(region.proj4())?;
    let mut values = vec![f64::NAN; size * size];
    let mut assets = vec![];
    let mut owners = vec![None; size * size];
    for (id, (path, asset)) in files.iter().enumerate() {
        let path = filename(path)?;
        let declared_datum = CString::new(asset.product.vertical_datum.as_str())?;
        let mut buffer = vec![f64::NAN; size * size];
        let mut error = [0; 2048];
        // SAFETY: dimensions are bounded and buffer has size*size elements.
        // C closes all handles and retains none of these pointers.
        let ok = unsafe {
            c2s_warp(
                path.as_ptr(),
                projection.as_ptr(),
                declared_datum.as_ptr(),
                size as c_int,
                region.radius_m,
                buffer.as_mut_ptr(),
                error.as_mut_ptr(),
                error.len(),
            )
        };
        status(ok, &error)?;
        let mut asset = asset.clone();
        asset.contributing_grid_nodes = 0;
        for ((dst, owner), src) in values.iter_mut().zip(&mut owners).zip(buffer) {
            if src.is_finite() {
                *dst = src;
                *owner = Some(id);
            }
        }
        assets.push(asset);
    }
    for id in owners.into_iter().flatten() {
        assets[id].contributing_grid_nodes += 1;
    }
    Ok((Grid::new(size, values)?, assets))
}
/// Explicit georeferenced writer, also used for reproducible test fixtures.
pub struct RasterSpec<'a> {
    pub projection: &'a str,
    pub width: usize,
    pub height: usize,
    pub affine: [f64; 6],
    pub values: &'a [f64],
    pub nodata: f64,
    pub scale: f64,
    pub offset: f64,
    pub datum: &'a str,
    pub unit: &'a str,
}
pub fn write_raster(path: &Path, s: &RasterSpec<'_>) -> Result<()> {
    ensure!(
        s.width > 0
            && s.height > 0
            && s.width <= 10000
            && s.height <= 10000
            && s.values.len() == s.width * s.height,
        "invalid raster dimensions"
    );
    register();
    let path = filename(path)?;
    let projection = CString::new(s.projection)?;
    let datum = CString::new(s.datum)?;
    let unit = CString::new(s.unit)?;
    let mut error = [0; 2048];
    // SAFETY: buffers outlive the call; their lengths were checked above.
    // The C shim owns and closes the output dataset.
    let ok = unsafe {
        c2s_write(
            path.as_ptr(),
            projection.as_ptr(),
            s.width as c_int,
            s.height as c_int,
            s.affine.as_ptr(),
            s.values.as_ptr(),
            s.nodata,
            s.scale,
            s.offset,
            datum.as_ptr(),
            unit.as_ptr(),
            error.as_mut_ptr(),
            error.len(),
        )
    };
    status(ok, &error)
}
pub fn save_grid(path: &Path, region: &Region, grid: &Grid, datum: &str) -> Result<()> {
    save_grid_with_options(path, region, grid, datum, false)
}
pub fn save_grid_with_options(
    path: &Path,
    region: &Region,
    grid: &Grid,
    datum: &str,
    overwrite: bool,
) -> Result<()> {
    region.validate()?;
    ensure!(
        (3..=2049).contains(&grid.size) && grid.values.len() == grid.size * grid.size,
        "invalid working grid"
    );
    ensure!(overwrite || !path.exists(), "saved grid exists");
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    let temp = tempfile::NamedTempFile::new_in(parent)?;
    let step = 2. * region.radius_m / (grid.size - 1) as f64;
    write_raster(
        temp.path(),
        &RasterSpec {
            projection: &region.proj4(),
            width: grid.size,
            height: grid.size,
            affine: [
                -region.radius_m - step / 2.,
                step,
                0.,
                region.radius_m + step / 2.,
                0.,
                -step,
            ],
            values: &grid.values,
            nodata: f64::NAN,
            scale: 1.,
            offset: 0.,
            datum,
            unit: "m",
        },
    )?;
    temp.as_file().sync_all()?;
    if overwrite {
        temp.persist(path).context("committing working GeoTIFF")?;
    } else {
        temp.persist_noclobber(path)
            .context("committing working GeoTIFF")?;
    }
    Ok(())
}
