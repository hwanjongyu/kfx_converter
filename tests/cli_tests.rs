//! Integration tests for `kfx-conv` CLI binary (`src/main.rs`).

mod common;

use std::process::Command;

fn get_bin() -> &'static str {
    env!("CARGO_BIN_EXE_kfx-conv")
}

#[test]
fn test_cli_missing_positional_args_returns_code_2() {
    let output = Command::new(get_bin())
        .output()
        .expect("Failed to execute binary");

    assert_eq!(
        output.status.code(),
        Some(2),
        "Clap must exit with code 2 on missing required positional arguments"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Usage") || stderr.contains("required"),
        "Stderr should contain usage help: {}",
        stderr
    );
}

#[test]
fn test_cli_invalid_option_returns_code_2() {
    let output = Command::new(get_bin())
        .arg("--unrecognized-invalid-option-xyz")
        .output()
        .expect("Failed to execute binary");

    assert_eq!(
        output.status.code(),
        Some(2),
        "Clap must exit with code 2 on unexpected arguments"
    );
}

#[test]
fn test_cli_help_flag() {
    let output = Command::new(get_bin())
        .arg("--help")
        .output()
        .expect("Failed to execute binary");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("High-performance native EPUB to Amazon KFX converter"));
    assert!(stdout.contains("--inspect"));
    assert!(stdout.contains("--output"));
    assert!(stdout.contains("--word-wise"));
    assert!(stdout.contains("--content-type"));
}

#[test]
fn test_cli_version_flag() {
    let output = Command::new(get_bin())
        .arg("--version")
        .output()
        .expect("Failed to execute binary");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("0.1.0"));
}

#[test]
fn test_cli_inspect_valid_kfx() {
    let temp = common::temp_dir();
    let kfx_path = temp.path().join("valid_sample.kfx");
    common::create_valid_kfx(&kfx_path);

    let output = Command::new(get_bin())
        .arg(&kfx_path)
        .arg("--inspect")
        .output()
        .expect("Failed to execute binary");

    assert!(
        output.status.success(),
        "Inspect on valid KFX should exit with code 0"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Valid KFX Container:"));
    assert!(stdout.contains("Version: 2"));
    assert!(stdout.contains("Total Chunks: 2"));
    assert!(stdout.contains("bcContId"));
    assert!(stdout.contains("section"));
}

#[test]
fn test_cli_inspect_nonexistent_file_returns_code_1() {
    let temp = common::temp_dir();
    let non_existent = temp.path().join("does_not_exist.kfx");

    let output = Command::new(get_bin())
        .arg(&non_existent)
        .arg("-i")
        .output()
        .expect("Failed to execute binary");

    assert_eq!(
        output.status.code(),
        Some(1),
        "Inspect on missing file should exit with code 1"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Failed to inspect KFX container"));
}

#[test]
fn test_cli_inspect_drm_protected_kfx_returns_code_1() {
    let temp = common::temp_dir();

    // Test secondary magic \xeaDRM
    let drm_path_1 = temp.path().join("drm_protected_1.kfx");
    common::create_drm_kfx(&drm_path_1, b"\xeaDRM");

    let output1 = Command::new(get_bin())
        .arg(&drm_path_1)
        .arg("-i")
        .output()
        .expect("Failed to execute binary");

    assert_eq!(output1.status.code(), Some(1));
    let stderr1 = String::from_utf8_lossy(&output1.stderr);
    assert!(stderr1.contains("DRM-protected KFX containers are not supported"));

    // Test secondary magic \xa8DRM
    let drm_path_2 = temp.path().join("drm_protected_2.kfx");
    common::create_drm_kfx(&drm_path_2, b"\xa8DRM");

    let output2 = Command::new(get_bin())
        .arg(&drm_path_2)
        .arg("--inspect")
        .output()
        .expect("Failed to execute binary");

    assert_eq!(output2.status.code(), Some(1));
    let stderr2 = String::from_utf8_lossy(&output2.stderr);
    assert!(stderr2.contains("DRM-protected KFX containers are not supported"));
}

#[test]
fn test_cli_inspect_corrupt_kfx_returns_code_1() {
    let temp = common::temp_dir();
    let corrupt_path = temp.path().join("corrupt.kfx");
    // Write truncated header (<18 bytes)
    std::fs::write(&corrupt_path, b"CONT_TRUNCATED").unwrap();

    let output = Command::new(get_bin())
        .arg(&corrupt_path)
        .arg("-i")
        .output()
        .expect("Failed to execute binary");

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("File too small to be a KFX container"));
}

#[test]
fn test_cli_convert_default_output() {
    let temp = common::temp_dir();
    let epub_path = common::create_minimal_epub(
        temp.path(),
        "default_out.epub",
        "Default Output Book",
        "en",
        false,
    );

    let output = Command::new(get_bin())
        .arg(&epub_path)
        .output()
        .expect("Failed to execute binary");

    assert!(output.status.success(), "Conversion should succeed");
    let expected_kfx = temp.path().join("default_out.kfx");
    assert!(
        expected_kfx.is_file(),
        "Default output file should be created"
    );
    let expected_sdr = temp.path().join("default_out.sdr");
    assert!(
        expected_sdr.is_dir(),
        "Companion SDR directory should exist"
    );
}

#[test]
fn test_cli_convert_custom_output_flag() {
    let temp = common::temp_dir();
    let epub_path = common::create_minimal_epub(
        temp.path(),
        "source.epub",
        "Custom Output Book",
        "en",
        false,
    );
    let custom_output = temp.path().join("custom_nested").join("custom_target.kfx");
    std::fs::create_dir_all(custom_output.parent().unwrap()).unwrap();

    let output = Command::new(get_bin())
        .arg(&epub_path)
        .arg("-o")
        .arg(&custom_output)
        .output()
        .expect("Failed to execute binary");

    assert!(output.status.success(), "Conversion should succeed");
    assert!(custom_output.is_file(), "Custom output path must exist");

    let companion_sdr = temp.path().join("custom_nested").join("custom_target.sdr");
    assert!(
        companion_sdr.is_dir(),
        "Companion SDR directory must exist beside custom output"
    );
}

#[test]
fn test_cli_convert_word_wise_toggle() {
    let temp = common::temp_dir();
    let epub_path =
        common::create_minimal_epub(temp.path(), "noww.epub", "No Word Wise Book", "en", false);
    let target_kfx = temp.path().join("noww.kfx");

    let output = Command::new(get_bin())
        .arg(&epub_path)
        .arg("-o")
        .arg(&target_kfx)
        .arg("--word-wise")
        .arg("false")
        .output()
        .expect("Failed to execute binary");

    assert!(
        output.status.success(),
        "Conversion without Word Wise should succeed"
    );
    assert!(target_kfx.is_file());

    let sdr_path = temp.path().join("noww.sdr");
    assert!(sdr_path.is_dir());

    // Verify NO .kll files exist when word_wise is false
    let mut kll_count = 0;
    for entry in std::fs::read_dir(&sdr_path).unwrap().flatten() {
        if entry.file_name().to_string_lossy().ends_with(".kll") {
            kll_count += 1;
        }
    }
    assert_eq!(
        kll_count, 0,
        "No KLL file should be created when --word-wise false is specified"
    );
}

#[test]
fn test_cli_convert_content_type_pdoc() {
    let temp = common::temp_dir();
    let epub_path =
        common::create_minimal_epub(temp.path(), "doc.epub", "Personal Document", "en", false);
    let target_kfx = temp.path().join("doc.kfx");

    let output = Command::new(get_bin())
        .arg(&epub_path)
        .arg("-o")
        .arg(&target_kfx)
        .arg("-c")
        .arg("PDOC")
        .output()
        .expect("Failed to execute binary");

    assert!(
        output.status.success(),
        "Conversion with PDOC should succeed"
    );
    assert!(target_kfx.is_file());
}

#[test]
fn test_cli_convert_missing_input_file_returns_code_1() {
    let output = Command::new(get_bin())
        .arg("/path/to/missing/book/that/does/not/exist.epub")
        .output()
        .expect("Failed to execute binary");

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Input file not found"),
        "Stderr should contain file not found error: {}",
        stderr
    );
}
