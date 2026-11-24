// build.rs - Automatic native library locator for Linux GUI (GPUI) and Windows PE resources

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_OS");
    println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_ARCH");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_GUI");

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let gui_enabled = std::env::var_os("CARGO_FEATURE_GUI").is_some();

    match target_os.as_str() {
        "linux" => {
            if gui_enabled {
                #[cfg(unix)]
                {
                    use std::path::{Path, PathBuf};

                    // On Linux GNU, GPUI links against -lxkbcommon-x11.
                    // If libxkbcommon-x11-dev is not installed, only libxkbcommon-x11.so.0 exists.
                    // Stage a symlink inside Cargo's OUT_DIR without mutating CARGO_MANIFEST_DIR.
                    let Some(out_dir) = std::env::var_os("OUT_DIR") else {
                        return;
                    };
                    let native_libs = PathBuf::from(out_dir).join("native-libs");
                    let symlink_path = native_libs.join("libxkbcommon-x11.so");

                    let target_arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
                    let system_candidates: &[&str] = match target_arch.as_str() {
                        "x86_64" => &[
                            "/usr/lib/x86_64-linux-gnu/libxkbcommon-x11.so.0",
                            "/usr/lib64/libxkbcommon-x11.so.0",
                            "/usr/lib/libxkbcommon-x11.so.0",
                        ],
                        "aarch64" => &[
                            "/usr/lib/aarch64-linux-gnu/libxkbcommon-x11.so.0",
                            "/usr/lib64/libxkbcommon-x11.so.0",
                            "/usr/lib/libxkbcommon-x11.so.0",
                        ],
                        _ => &[
                            "/usr/lib/x86_64-linux-gnu/libxkbcommon-x11.so.0",
                            "/usr/lib/aarch64-linux-gnu/libxkbcommon-x11.so.0",
                            "/usr/lib64/libxkbcommon-x11.so.0",
                            "/usr/lib/libxkbcommon-x11.so.0",
                        ],
                    };

                    if let Some(cand_path) =
                        system_candidates.iter().map(Path::new).find(|p| p.exists())
                    {
                        let needs_symlink = match std::fs::symlink_metadata(&symlink_path) {
                            Ok(meta) => {
                                let valid_same_target = meta.file_type().is_symlink()
                                    && std::fs::read_link(&symlink_path).ok().as_deref()
                                        == Some(cand_path)
                                    && symlink_path.exists();
                                if !valid_same_target {
                                    let _ = std::fs::remove_file(&symlink_path);
                                    true
                                } else {
                                    false
                                }
                            }
                            Err(_) => true,
                        };

                        if needs_symlink && std::fs::create_dir_all(&native_libs).is_ok() {
                            let _ = std::os::unix::fs::symlink(cand_path, &symlink_path);
                        }
                    } else if let Ok(meta) = std::fs::symlink_metadata(&symlink_path)
                        && meta.file_type().is_symlink()
                        && !symlink_path.exists()
                    {
                        let _ = std::fs::remove_file(&symlink_path);
                    }

                    if let Ok(meta) = std::fs::symlink_metadata(&symlink_path)
                        && !meta.is_dir()
                        && symlink_path.exists()
                    {
                        println!("cargo:rustc-link-search=native={}", native_libs.display());
                    }
                }
            }
        }
        "windows" => {
            let file_desc = if gui_enabled {
                "Native EPUB to Amazon KFX Converter & Kindle Studio"
            } else {
                "Native EPUB to Amazon KFX Converter CLI"
            };
            #[cfg(windows)]
            {
                let mut res = winres::WindowsResource::new();
                res.set("FileDescription", file_desc);
                res.set("ProductName", "kfx-converter");
                res.set("LegalCopyright", "Copyright (c) 2026 hwanjongyu");
                res.set("CompanyName", env!("CARGO_PKG_AUTHORS"));
                res.set("ProductVersion", env!("CARGO_PKG_VERSION"));
                res.set("FileVersion", env!("CARGO_PKG_VERSION"));
                // Note: Omit OriginalFilename so kfx-conv.exe is never mislabeled as kfx-studio.exe.
                // Note: Do NOT call res.set_manifest(...) here.
                // `gpui` already embeds its own application manifest (DPI awareness & common controls).
                // Calling set_manifest results in CVTRES fatal error CVT1100 (duplicate resource type:MANIFEST, name:1).
                if let Err(e) = res.compile() {
                    eprintln!("cargo:warning=Failed to compile Windows resources: {}", e);
                }
            }
            #[cfg(not(windows))]
            let _ = file_desc;
        }
        _ => {}
    }
}
