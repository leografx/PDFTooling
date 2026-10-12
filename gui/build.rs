//! Windows builds: embed the app icon and version info in the .exe, so Explorer,
//! the taskbar and Alt-Tab show it. Other platforms: nothing to do.

#[path = "src/iconpix.rs"]
mod iconpix;

fn main() {
    println!("cargo:rerun-if-changed=src/iconpix.rs");
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR")).join("impose-settings.ico");
    if let Err(e) = std::fs::write(&out, iconpix::ico(&[16, 24, 32, 48, 64, 256])) {
        println!("cargo:warning=couldn't write the app icon ({e}); the .exe will have the default icon");
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon(&out.to_string_lossy())
        .set("ProductName", "Impose Settings")
        .set("FileDescription", "Impose Settings")
        .set("InternalName", "impose-settings")
        .set("OriginalFilename", "impose-settings.exe");
    // A missing resource compiler only costs the Explorer icon, never the build.
    if let Err(e) = res.compile() {
        println!("cargo:warning=couldn't embed the app icon ({e}); the .exe will have the default icon");
    }
}
