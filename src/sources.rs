//! Version-pinned official products and an integrity-checked, bounded cache.
use anyhow::{Context, Result, bail, ensure};
use geographiclib_rs::{DirectGeodesic, Geodesic, InverseGeodesic};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Region {
    pub latitude: f64,
    pub longitude: f64,
    pub radius_m: f64,
}
impl Region {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.latitude.is_finite() && (-90. ..=90.).contains(&self.latitude),
            "latitude must be -90..90 degrees"
        );
        ensure!(
            self.longitude.is_finite() && (-180. ..=180.).contains(&self.longitude),
            "longitude must be -180..180 degrees"
        );
        ensure!(
            self.radius_m.is_finite() && (1000. ..=1_000_000.).contains(&self.radius_m),
            "radius must be 1..1000 km"
        );
        Ok(())
    }
    pub fn proj4(&self) -> String {
        format!(
            "+proj=aeqd +lat_0={} +lon_0={} +ellps=WGS84 +units=m +no_defs +type=crs",
            self.latitude, self.longitude
        )
    }
    pub fn geographic(&self, x_m: f64, y_m: f64) -> (f64, f64) {
        let azimuth = x_m.atan2(y_m).to_degrees();
        Geodesic::wgs84().direct(self.latitude, self.longitude, azimuth, x_m.hypot(y_m))
    }
    pub fn projected(&self, latitude: f64, longitude: f64) -> (f64, f64) {
        let (distance, azimuth, _, _): (f64, f64, f64, f64) =
            Geodesic::wgs84().inverse(self.latitude, self.longitude, latitude, longitude);
        (
            distance * azimuth.to_radians().sin(),
            distance * azimuth.to_radians().cos(),
        )
    }
    /// Maximum of 360 sampled boundary tangential scales, s / reduced length.
    /// This is a sampled diagnostic, not a rigorous global distortion bound.
    pub fn sampled_tangential_scale(&self) -> f64 {
        (0..360)
            .map(|azimuth| {
                let (_, _, _, reduced, _, _): (f64, f64, f64, f64, f64, f64) = Geodesic::wgs84()
                    .direct(self.latitude, self.longitude, azimuth as f64, self.radius_m);
                self.radius_m / reduced
            })
            .fold(1., f64::max)
    }
    /// Conservative ellipsoidal bounds, split at the date line. R_min bounds
    /// the ellipsoid's metric below; a spherical cap with r/R_min contains it.
    pub fn bounds(&self, halo_m: f64) -> Vec<[f64; 4]> {
        let delta = (self.radius_m + halo_m) / 6_335_000.;
        let phi = self.latitude.to_radians();
        let south = (self.latitude - delta.to_degrees()).max(-90.);
        let north = (self.latitude + delta.to_degrees()).min(90.);
        if phi.abs() + delta >= std::f64::consts::FRAC_PI_2 {
            return vec![[-180., south, 180., north]];
        }
        let half = (delta.sin() / phi.cos()).clamp(-1., 1.).asin().to_degrees();
        let west = self.longitude - half;
        let east = self.longitude + half;
        if west < -180. {
            vec![
                [west + 360., south, 180., north],
                [-180., south, east, north],
            ]
        } else if east > 180. {
            vec![
                [west, south, 180., north],
                [-180., south, east - 360., north],
            ]
        } else {
            vec![[west, south, east, north]]
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Product {
    pub id: String,
    pub provider: String,
    pub url: String,
    pub attribution: String,
    pub vertical_datum: String,
    pub native_spacing_arcseconds: Option<f64>,
    pub surface_type: String,
    pub published_vertical_accuracy: Option<String>,
}
pub fn etopo_products(region: &Region, halo_m: f64) -> Vec<Product> {
    let boxes = region.bounds(halo_m);
    let mut out = vec![];
    for north in (-75_i32..=90).step_by(15) {
        for west in (-180_i32..180).step_by(15) {
            if !boxes.iter().any(|b| {
                (west as f64) < b[2]
                    && ((west + 15) as f64) > b[0]
                    && ((north - 15) as f64) < b[3]
                    && (north as f64) > b[1]
            }) {
                continue;
            }
            let ns = if north >= 0 { 'N' } else { 'S' };
            let ew = if west >= 0 { 'E' } else { 'W' };
            let id = format!(
                "ETOPO_2022_v1_15s_{ns}{:02}{ew}{:03}_surface",
                north.abs(),
                west.abs()
            );
            out.push(Product{id:id.clone(),provider:"NOAA ETOPO 2022 v1".into(),
            url:format!("https://www.ngdc.noaa.gov/mgg/global/relief/ETOPO2022/data/15s/15s_surface_elev_gtif/{id}.tif"),
            attribution:"NOAA National Centers for Environmental Information (2022), ETOPO 2022 15 Arc-Second Global Relief Model. https://doi.org/10.25921/fd45-gt74".into(),
            vertical_datum:"EGM2008".into(),native_spacing_arcseconds:Some(15.),surface_type:"land terrain / seabed / ice surface".into(),published_vertical_accuracy:None});
        }
    }
    out
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Asset {
    pub product: Product,
    pub sha256: String,
    pub bytes: u64,
    pub contributing_grid_nodes: usize,
}
pub struct Cache {
    root: PathBuf,
    client: Client,
    pub offline: bool,
    pub max_download_bytes: u64,
    pub downloaded_bytes: u64,
}
impl Cache {
    pub fn new(root: PathBuf, offline: bool, max_download_bytes: u64) -> Result<Self> {
        let client = Client::builder()
            .user_agent(concat!("coord2svg/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(120))
            .build()?;
        Ok(Self {
            root,
            client,
            offline,
            max_download_bytes,
            downloaded_bytes: 0,
        })
    }
    pub fn paths(&self, product: &Product) -> (PathBuf, PathBuf) {
        let key = format!("{:x}", Sha256::digest(product.url.as_bytes()));
        (
            self.root.join(format!("{key}.tif")),
            self.root.join(format!("{key}.json")),
        )
    }
    pub fn get(&mut self, product: &Product) -> Result<(PathBuf, Asset)> {
        let (path, meta) = self.paths(product);
        if path.exists() && meta.exists() {
            let mut asset: Asset = serde_json::from_slice(&fs::read(&meta)?)?;
            ensure!(
                asset.product.url == product.url,
                "cache URL mismatch: {}",
                meta.display()
            );
            let (hash, bytes) = checksum(&path)?;
            ensure!(
                hash == asset.sha256 && bytes == asset.bytes,
                "cache checksum mismatch: {}; remove this cached file and its JSON sidecar to fetch again",
                path.display()
            );
            asset.product = product.clone();
            asset.contributing_grid_nodes = 0;
            return Ok((path, asset));
        }
        ensure!(!self.offline, "offline cache miss for {}", product.id);
        fs::create_dir_all(&self.root)?;
        let temp = path.with_extension(format!("part.{}", std::process::id()));
        let result = (|| -> Result<Asset> {
            let mut response = None;
            for attempt in 0..3 {
                match self.client.get(&product.url).send() {
                    Ok(r) if r.status().is_success() => {
                        response = Some(r);
                        break;
                    }
                    Ok(r) if !r.status().is_server_error() && r.status().as_u16() != 429 => {
                        bail!("HTTP {} downloading {}", r.status(), product.url)
                    }
                    Ok(r) if attempt == 2 => bail!(
                        "HTTP {} after retries downloading {}",
                        r.status(),
                        product.url
                    ),
                    Err(e) if attempt == 2 => {
                        return Err(e).context(format!("downloading {}", product.url));
                    }
                    _ => std::thread::sleep(Duration::from_millis(250 * (attempt + 1))),
                }
            }
            let mut response = response.context("no download response")?;
            let remaining = self
                .max_download_bytes
                .saturating_sub(self.downloaded_bytes);
            let limit = remaining.min(128 * 1024 * 1024);
            if let Some(size) = response.content_length() {
                ensure!(
                    size <= limit,
                    "download budget exceeded for {} ({} bytes, {} remaining)",
                    product.id,
                    size,
                    limit
                );
            }
            let mut file = File::create(&temp)?;
            let mut hash = Sha256::new();
            let mut bytes = 0;
            let mut buffer = [0u8; 64 * 1024];
            loop {
                let n = response.read(&mut buffer)?;
                if n == 0 {
                    break;
                }
                bytes += n as u64;
                ensure!(
                    bytes <= limit,
                    "download budget exceeded while fetching {}",
                    product.id
                );
                file.write_all(&buffer[..n])?;
                hash.update(&buffer[..n]);
            }
            ensure!(bytes > 0, "empty response for {}", product.id);
            file.sync_all()?;
            // Refuse HTML error pages before committing to the cache.
            crate::raster::validate(&temp)
                .context("download is not a readable single-band GDAL raster")?;
            Ok(Asset {
                product: product.clone(),
                sha256: format!("{:x}", hash.finalize()),
                bytes,
                contributing_grid_nodes: 0,
            })
        })();
        match result {
            Ok(asset) => {
                fs::rename(&temp, &path)?;
                let meta_temp = meta.with_extension(format!("part.{}", std::process::id()));
                fs::write(&meta_temp, serde_json::to_vec_pretty(&asset)?)?;
                fs::rename(&meta_temp, &meta)?;
                self.downloaded_bytes += asset.bytes;
                Ok((path, asset))
            }
            Err(e) => {
                let _ = fs::remove_file(&temp);
                Err(e)
            }
        }
    }
}
pub fn checksum(path: &Path) -> Result<(String, u64)> {
    let mut file = File::open(path)?;
    let mut hash = Sha256::new();
    let mut bytes = 0;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
        bytes += n as u64;
    }
    Ok((format!("{:x}", hash.finalize()), bytes))
}
