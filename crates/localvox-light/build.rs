/// Windows: embed the icon into the exe plus a "tray-icon" resource for the system tray
/// (`--tray`, WP-C3). macOS: embed the daemon's capture usage descriptions.
fn main() {
    let manifest_dir = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let plist = manifest_dir.join("../../assets/macos/daemon-Info.plist");
    println!("cargo:rerun-if-changed={}", plist.display());
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        let version = std::env::var("CARGO_PKG_VERSION").unwrap();
        let contents = std::fs::read_to_string(&plist)
            .expect("macOS capture usage descriptions")
            .replace("@VERSION@", &version);
        let output = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap())
            .join("daemon-Info.plist");
        std::fs::write(&output, contents).expect("write embedded daemon Info.plist");
        println!(
            "cargo:rustc-link-arg-bin=localvox-light=-Wl,-sectcreate,__TEXT,__info_plist,{}",
            output.display()
        );
    }
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
