use std::path::{Path, PathBuf};

fn profile_bin_dir() -> PathBuf {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let target_root = std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| manifest.join("../../target"));
    let profile = std::env::var("PROFILE").unwrap();
    let target_triple = std::env::var("TARGET").unwrap();
    let host = std::env::var("HOST").unwrap();
    if target_triple == host {
        target_root.join(profile)
    } else {
        target_root.join(target_triple).join(profile)
    }
}

fn copy_vosk_dlls(vosk_lib: &Path, dest: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(vosk_lib)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("dll") {
            continue;
        }
        let name = match path.file_name() {
            Some(n) => n,
            None => continue,
        };
        let to = dest.join(name);
        std::fs::copy(&path, &to)?;
        println!("cargo:rerun-if-changed={}", path.display());
    }
    Ok(())
}

fn workspace_vosk_lib(manifest_dir: &Path) -> PathBuf {
    manifest_dir.join("..").join("..").join("vosk-lib")
}

fn main() {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let vosk_lib = workspace_vosk_lib(&manifest_dir);
    if vosk_lib.exists() {
        println!("cargo:rustc-link-search=native={}", vosk_lib.display());
    }

    let target = std::env::var("TARGET").unwrap_or_default();
    if target.contains("windows") && vosk_lib.is_dir() {
        let dest = profile_bin_dir();
        match copy_vosk_dlls(&vosk_lib, &dest) {
            Ok(()) => {}
            Err(e) => println!(
                "cargo:warning=Не удалось скопировать DLL из vosk-lib в {}: {e}. Добавьте vosk-lib в PATH.",
                dest.display()
            ),
        }
    }

    println!("cargo:rerun-if-changed=build.rs");
}
