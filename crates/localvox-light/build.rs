/// Windows: embed the icon into the exe plus a "tray-icon" resource for the system tray
/// (`--tray`, WP-C3). On other systems — a no-op.
fn main() {
    #[cfg(target_os = "windows")]
    {
        let icon = "../../assets/localvox.ico";
        if std::path::Path::new(icon).exists() {
            let mut res = winres::WindowsResource::new();
            res.set_icon(icon); // the exe icon
            res.set_icon_with_id(icon, "tray-icon"); // the resource for the tray item
            if let Err(e) = res.compile() {
                println!("cargo:warning=winres: {e}");
            }
        } else {
            println!("cargo:warning=assets/localvox.ico is missing — the tray will not build without an icon");
        }
        println!("cargo:rerun-if-changed=../../assets/localvox.ico");
    }
}
