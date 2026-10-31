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
        res.set_manifest(r#"
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
<trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
        <requestedPrivileges>
            <requestedExecutionLevel level="asInvoker" uiAccess="false" />
        </requestedPrivileges>
    </security>
</trustInfo>
<compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
        <!-- Windows 10 and Windows 11 -->
        <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>
        <!-- Windows 8.1 -->
        <supportedOS Id="{1f676c76-80e1-4239-95bb-83d0f6d0da78}"/>
        <!-- Windows 8 -->
        <supportedOS Id="{4a2f28e3-53b9-4441-ba9c-d69d4a4a6e38}"/>
        <!-- Windows 7 -->
        <supportedOS Id="{35138b9a-5d96-4fbd-8e2d-a2440225f93a}"/>
    </application>
</compatibility>
</assembly>
"#);
        if let Err(e) = res.compile() {
            eprintln!("cargo:warning=Failed to compile Windows resources: {}", e);
        }
    }
}
