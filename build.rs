// rust/build.rs - Automatic native library locator for Linux GUI (GPUI) and Windows PE resources

fn main() {
    #[cfg(target_os = "linux")]
    {
        use std::path::Path;

        // On Linux GNU, GPUI links against -lxkbcommon-x11.
        // If libxkbcommon-x11-dev is not installed, only libxkbcommon-x11.so.0 exists.
        // We ensure a local symlink exists and instruct rustc to search native-libs.
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let native_libs = Path::new(&manifest_dir).join("native-libs");
        let symlink_path = native_libs.join("libxkbcommon-x11.so");

        let system_candidates = [
            "/usr/lib/x86_64-linux-gnu/libxkbcommon-x11.so.0",
            "/usr/lib64/libxkbcommon-x11.so.0",
            "/usr/lib/libxkbcommon-x11.so.0",
        ];

        if !symlink_path.exists() {
            for cand in &system_candidates {
                let cand_path = Path::new(cand);
                if cand_path.exists() {
                    let _ = std::fs::create_dir_all(&native_libs);
                    #[cfg(unix)]
                    let _ = std::os::unix::fs::symlink(cand_path, &symlink_path);
                    break;
                }
            }
        }

        if native_libs.exists() {
            println!("cargo:rustc-link-search=native={}", native_libs.display());
        }
    }

    #[cfg(target_os = "windows")]
    {
        let mut res = winres::WindowsResource::new();
        res.set("FileDescription", "Kindle Scribe Studio / EPUB to KFX Converter");
        res.set("ProductName", "Kindle Scribe Studio");
        res.set("OriginalFilename", "kfx-studio.exe");
        res.set("LegalCopyright", "Copyright (c) 2026 hwanjongyu");
        res.set("CompanyName", "hwanjongyu");
        res.set("ProductVersion", "0.1.0");
        res.set("FileVersion", "0.1.0");
        // Note: Do NOT call res.set_manifest(...) here.
        // `gpui` already embeds its own application manifest (DPI awareness & common controls).
        // Calling set_manifest results in CVTRES fatal error CVT1100 (duplicate resource type:MANIFEST, name:1).
        if let Err(e) = res.compile() {
            eprintln!("cargo:warning=Failed to compile Windows resources: {}", e);
        }
    }
}
