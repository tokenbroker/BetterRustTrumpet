//! Embeds the application icon (resource 1, also the tray icon) and the
//! manifest: per-monitor DPI awareness and common controls 6.
fn main() {
    println!("cargo:rerun-if-changed=icons/icon.ico");
    println!("cargo:rerun-if-changed=app.manifest");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("icons/icon.ico");
        resource.set_manifest(include_str!("app.manifest"));
        resource.compile().expect("compile Windows resources");
    }
}
