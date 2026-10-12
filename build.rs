#[path = "src/icon.rs"]
#[allow(dead_code)]
mod icon;

use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=src/icon.rs");
    println!("cargo:rerun-if-changed=build.rs");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let ico_path = out_dir.join("app.ico");
    std::fs::write(&ico_path, icon::ico_bytes(&[16, 24, 32, 48, 64, 128, 256])).unwrap();

    let mut res = winresource::WindowsResource::new();
    res.set_icon(ico_path.to_str().unwrap());
    res.set("ProductName", "Cardboard Capital");
    res.set("FileDescription", "Cardboard Capital");
    if let Err(e) = res.compile() {
        // A missing resource compiler shouldn't block the build; the exe just won't have an icon.
        println!("cargo:warning=could not embed exe icon: {e}");
    }
}
