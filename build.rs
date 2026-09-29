fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    #[cfg(all(windows, feature = "view"))]
    windows_icon();
}

/// Windows shows resource 1 of the executable in Explorer, the taskbar, and
/// the window title bar.
#[cfg(all(windows, feature = "view"))]
fn windows_icon() {
    use std::path::{Path, PathBuf};

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let root = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let ico = root.join("assets").join("icon").join("tgw.ico");
    println!("cargo:rerun-if-changed={}", ico.display());
    let rc = Path::new(&std::env::var_os("OUT_DIR").unwrap()).join("tgw.rc");
    let escaped = ico.display().to_string().replace('\\', "\\\\");
    std::fs::write(&rc, format!("1 ICON \"{escaped}\"\n")).unwrap();
    if let Err(error) =
        embed_resource::compile_for(&rc, ["tgw"], embed_resource::NONE).manifest_optional()
    {
        println!("cargo:warning=tgw icon not embedded: {error}");
    }
}
