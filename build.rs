// rust/build.rs - Automatic native library locator for Linux GUI (GPUI)

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
}
