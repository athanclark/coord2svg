use coord2svg::sources::{Asset, Cache, Product, Region, checksum, etopo_products};
use std::{
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    thread,
    time::{Duration, Instant},
};

fn region(lat: f64, lon: f64, r: f64) -> Region {
    Region {
        latitude: lat,
        longitude: lon,
        radius_m: r,
    }
}
#[test]
fn hawaii_products_are_pinned() {
    let p = etopo_products(&region(20.9, -157.4, 500_000.), 1000.);
    assert!(
        p.iter()
            .any(|p| p.id == "ETOPO_2022_v1_15s_N30W165_surface")
    );
    assert!(
        p.iter()
            .all(|p| p.url.starts_with("https://www.ngdc.noaa.gov/")
                && p.vertical_datum == "EGM2008")
    );
}
#[test]
fn date_line_fetches_both_sides() {
    let p = etopo_products(&region(0., 179.9, 100_000.), 1000.);
    assert!(p.iter().any(|p| p.id.contains("E165")));
    assert!(p.iter().any(|p| p.id.contains("W180")));
    assert!(p.len() <= 4);
}
#[test]
fn poles_fetch_all_longitudes() {
    let n = etopo_products(&region(90., 0., 1000.), 10.);
    assert_eq!(n.len(), 24);
    assert!(n.iter().all(|p| p.id.contains("N90")));
    let s = etopo_products(&region(-90., 0., 1000.), 10.);
    assert_eq!(s.len(), 24);
    assert!(s.iter().all(|p| p.id.contains("S75")));
}
#[test]
fn geodesic_roundtrips_at_all_radii() {
    for lat in [-90., -80., 0., 21., 80., 90.] {
        for lon in [-180., -157., 0., 180.] {
            for r in [1000., 100_000., 1_000_000.] {
                let reg = region(lat, lon, r);
                for a in [0f64, 45., 90., 180., 270.] {
                    let (x, y) = (r * a.to_radians().sin(), r * a.to_radians().cos());
                    let (lat, lon) = reg.geographic(x, y);
                    let (xx, yy) = reg.projected(lat, lon);
                    assert!(
                        (xx - x).hypot(yy - y) < 1e-5,
                        "{reg:?}: {x},{y} -> {xx},{yy}"
                    );
                    assert!(
                        reg.bounds(1.)
                            .iter()
                            .any(|b| lon >= b[0] && lon <= b[2] && lat >= b[1] && lat <= b[3])
                    );
                }
            }
        }
    }
}
#[test]
fn invalid_coordinates_and_radii() {
    for r in [0., 999., 1_000_001., f64::NAN, f64::INFINITY] {
        assert!(region(0., 0., r).validate().is_err());
    }
    assert!(region(91., 0., 1000.).validate().is_err());
    assert!(region(0., 181., 1000.).validate().is_err());
}
#[test]
fn offline_cache_miss_is_actionable() {
    let dir = tempfile::tempdir().unwrap();
    let mut cache = Cache::new(dir.path().into(), true, 0).unwrap();
    let p = etopo_products(&region(21., -157., 1000.), 1.).remove(0);
    assert!(
        cache
            .get(&p)
            .unwrap_err()
            .to_string()
            .contains("offline cache miss")
    );
}
#[test]
fn cache_checksums_and_corruption() {
    let dir = tempfile::tempdir().unwrap();
    let mut cache = Cache::new(dir.path().into(), true, 0).unwrap();
    let p = etopo_products(&region(21., -157., 1000.), 1.).remove(0);
    let (path, meta) = cache.paths(&p);
    fs::write(&path, b"cached fixture").unwrap();
    let (sha256, bytes) = checksum(&path).unwrap();
    let a = Asset {
        product: p.clone(),
        sha256,
        bytes,
        contributing_grid_nodes: 0,
    };
    fs::write(&meta, serde_json::to_vec(&a).unwrap()).unwrap();
    assert_eq!(cache.get(&p).unwrap().1.bytes, 14);
    fs::write(&path, b"corruption").unwrap();
    assert!(
        cache
            .get(&p)
            .unwrap_err()
            .to_string()
            .contains("checksum mismatch")
    );
}
// Bound fixture waits so a broken downloader fails rather than hanging CI.
fn accept(listener: &TcpListener) -> TcpStream {
    listener.set_nonblocking(true).unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                return stream;
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(
                    Instant::now() < deadline,
                    "HTTP fixture timed out waiting for the downloader"
                );
                thread::sleep(Duration::from_millis(10));
            }
            Err(e) => panic!("HTTP fixture accept failed: {e}"),
        }
    }
}
fn server(body: Vec<u8>, code: &'static str) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/tile.tif", listener.local_addr().unwrap());
    let handle = thread::spawn(move || {
        let mut socket = accept(&listener);
        let mut req = [0; 4096];
        let count = socket.read(&mut req).unwrap();
        assert!(count > 0);
        write!(
            socket,
            "HTTP/1.1 {code}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .unwrap();
        let _ = socket.write_all(&body);
    });
    (url, handle)
}
fn product(url: String) -> Product {
    Product {
        id: "fixture".into(),
        provider: "test".into(),
        url,
        attribution: "test".into(),
        vertical_datum: "EGM2008".into(),
        native_spacing_arcseconds: None,
        surface_type: "test".into(),
        published_vertical_accuracy: None,
    }
}
#[test]
fn failed_download_does_not_commit_cache() {
    let dir = tempfile::tempdir().unwrap();
    let (url, handle) = server(b"not found".to_vec(), "404 Not Found");
    let p = product(url);
    let mut cache = Cache::new(dir.path().into(), false, 1000).unwrap();
    assert!(cache.get(&p).unwrap_err().to_string().contains("404"));
    handle.join().unwrap();
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
}
#[test]
fn download_limit_checked_before_commit() {
    let dir = tempfile::tempdir().unwrap();
    let (url, handle) = server(vec![0; 100], "200 OK");
    let p = product(url);
    let mut cache = Cache::new(dir.path().into(), false, 50).unwrap();
    assert!(cache.get(&p).unwrap_err().to_string().contains("budget"));
    handle.join().unwrap();
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
}

fn tile_bytes() -> Vec<u8> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tile.tif");
    let grid = coord2svg::terrain::Grid::analytic(3, |_, _| 10.).unwrap();
    coord2svg::raster::save_grid(&path, &region(0., 0., 1000.), &grid, "EGM2008").unwrap();
    fs::read(path).unwrap()
}
#[test]
fn valid_download_is_cached_and_reused_offline() {
    let dir = tempfile::tempdir().unwrap();
    let bytes = tile_bytes();
    let (url, handle) = server(bytes.clone(), "200 OK");
    let p = product(url);
    let mut cache = Cache::new(dir.path().into(), false, 10000).unwrap();
    let (path, a) = cache.get(&p).unwrap();
    handle.join().unwrap();
    assert_eq!(a.bytes, bytes.len() as u64);
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert_eq!(cache.downloaded_bytes, a.bytes);
    cache.offline = true;
    assert_eq!(cache.get(&p).unwrap().1.sha256, a.sha256);
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
}
#[test]
fn html_error_payload_is_not_cached() {
    let dir = tempfile::tempdir().unwrap();
    let (url, handle) = server(b"<html>error</html>".to_vec(), "200 OK");
    let mut cache = Cache::new(dir.path().into(), false, 10000).unwrap();
    assert!(cache.get(&product(url)).is_err());
    handle.join().unwrap();
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
}
#[test]
fn transient_http_errors_retry_then_succeed() {
    let dir = tempfile::tempdir().unwrap();
    let body = tile_bytes();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/tile.tif", listener.local_addr().unwrap());
    let handle = thread::spawn(move || {
        for i in 0..3 {
            let mut socket = accept(&listener);
            let mut request = [0u8; 4096];
            let n = socket.read(&mut request).unwrap();
            assert!(n > 0);
            let (code, payload) = if i < 2 {
                ("503 Service Unavailable", &b"retry"[..])
            } else {
                ("200 OK", body.as_slice())
            };
            write!(
                socket,
                "HTTP/1.1 {code}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                payload.len()
            )
            .unwrap();
            let _ = socket.write_all(payload);
        }
    });
    let mut cache = Cache::new(dir.path().into(), false, 10000).unwrap();
    assert!(cache.get(&product(url)).is_ok());
    handle.join().unwrap();
}
