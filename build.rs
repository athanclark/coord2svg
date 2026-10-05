fn main() {
    println!("cargo:rerun-if-changed=native/raster.c");
    println!("cargo:rerun-if-env-changed=GDAL_INCLUDE_DIR");
    println!("cargo:rerun-if-env-changed=GDAL_LIB_DIR");
    let mut build = cc::Build::new();
    build.file("native/raster.c");
    if let Ok(include) = std::env::var("GDAL_INCLUDE_DIR") {
        build.include(include);
        if let Ok(lib) = std::env::var("GDAL_LIB_DIR") {
            println!("cargo:rustc-link-search=native={lib}");
        }
        println!("cargo:rustc-link-lib=gdal");
    } else {
        let lib=pkg_config::Config::new().atleast_version("3.4").probe("gdal")
            .expect("GDAL development files are required. Install libgdal-dev (Linux) or gdal (Homebrew), or set GDAL_INCLUDE_DIR and GDAL_LIB_DIR.");
        for include in lib.include_paths {
            build.include(include);
        }
    }
    build.compile("coord2svg_raster");
}
