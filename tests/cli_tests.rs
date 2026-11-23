//! Integration tests for `kfx-conv` CLI binary (`src/main.rs`).

mod common;

use std::process::Command;

fn get_bin() -> &'static str {
    env!("CARGO_BIN_EXE_kfx-conv")
}

fn cli_cmd(log_dir: &std::path::Path) -> Command {
    let mut cmd = Command::new(get_bin());
    cmd.env("XDG_DATA_HOME", log_dir)
        .env("LOCALAPPDATA", log_dir);
    cmd
}

#[test]
fn test_cli_missing_positional_args_returns_code_2() {
    let temp = common::temp_dir();
    let output = cli_cmd(temp.path())
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
    let temp = common::temp_dir();
    let output = cli_cmd(temp.path())
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
    let temp = common::temp_dir();
    let output = cli_cmd(temp.path())
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
    let temp = common::temp_dir();
    let output = cli_cmd(temp.path())
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

    let output = cli_cmd(temp.path())
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

    let output = cli_cmd(temp.path())
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

    let output1 = cli_cmd(temp.path())
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

    let output2 = cli_cmd(temp.path())
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

    let output = cli_cmd(temp.path())
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

    let output = cli_cmd(temp.path())
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
    let isolated_log = temp.path().join("kfx-converter").join("conversion.log");
    assert!(
        isolated_log.is_file(),
        "CLI test log must be written inside isolated temp dir"
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

    let output = cli_cmd(temp.path())
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

    let output = cli_cmd(temp.path())
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

    let output = cli_cmd(temp.path())
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
    let temp = common::temp_dir();
    let output = cli_cmd(temp.path())
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

#[test]
fn test_cli_help_shows_word_wise_and_content_type_ebok() {
    let temp = common::temp_dir();
    let output = cli_cmd(temp.path())
        .arg("--help")
        .output()
        .expect("Failed to execute binary");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--content-type"));
    assert!(stdout.contains("EBOK"));
    assert!(stdout.contains("PDOC"));
    assert!(stdout.contains("--word-wise"));
}

#[test]
fn test_cli_short_word_wise_flag_with_positional_input() {
    let temp = common::temp_dir();
    let epub_path =
        common::create_minimal_epub(temp.path(), "ww_short.epub", "Word Wise Short", "en", false);

    let output = cli_cmd(temp.path())
        .arg("-w")
        .arg(&epub_path)
        .output()
        .expect("Failed to execute binary");

    assert!(
        output.status.success(),
        "kfx-conv -w <book.epub> must succeed without treating book path as -w value; stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(temp.path().join("ww_short.kfx").is_file());
    assert!(temp.path().join("ww_short.sdr").is_dir());
}

#[test]
fn test_cli_word_wise_equals_and_short_space_false() {
    let temp = common::temp_dir();
    let epub_path =
        common::create_minimal_epub(temp.path(), "ww_off.epub", "No WW Modes", "en", false);

    let out_eq = temp.path().join("eq_off.kfx");
    let res_eq = cli_cmd(temp.path())
        .arg(&epub_path)
        .arg("-o")
        .arg(&out_eq)
        .arg("--word-wise=false")
        .output()
        .expect("Failed to execute binary");
    assert!(res_eq.status.success());
    assert!(out_eq.is_file());

    let out_short = temp.path().join("short_off.kfx");
    let res_short = cli_cmd(temp.path())
        .arg(&epub_path)
        .arg("-o")
        .arg(&out_short)
        .arg("-w")
        .arg("false")
        .output()
        .expect("Failed to execute binary");
    assert!(res_short.status.success());
    assert!(out_short.is_file());

    for sdr_name in ["eq_off.sdr", "short_off.sdr"] {
        let kll_count = std::fs::read_dir(temp.path().join(sdr_name))
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".kll"))
            .count();
        assert_eq!(kll_count, 0, "Expected 0 .kll files in {sdr_name}");
    }
}

#[test]
fn test_cli_content_type_case_insensitive_and_invalid_exit_2() {
    let temp = common::temp_dir();
    let epub_path =
        common::create_minimal_epub(temp.path(), "ctype.epub", "Content Type Book", "en", false);
    let target_kfx = temp.path().join("ctype.kfx");

    let ok_lower = cli_cmd(temp.path())
        .arg(&epub_path)
        .arg("-o")
        .arg(&target_kfx)
        .arg("-c")
        .arg("pdoc")
        .output()
        .expect("Failed to execute binary");
    assert!(ok_lower.status.success(), "-c pdoc should succeed");

    let invalid = cli_cmd(temp.path())
        .arg(&epub_path)
        .arg("-c")
        .arg("INVALID")
        .output()
        .expect("Failed to execute binary");
    assert_eq!(
        invalid.status.code(),
        Some(2),
        "Invalid -c value must fail at Clap parse time with exit code 2"
    );
}

#[test]
fn test_cli_word_wise_error_emits_stderr_warning_and_no_success_line() {
    let temp = common::temp_dir();
    let epub_path =
        common::create_minimal_epub(temp.path(), "ww_warn.epub", "WW Warning Book", "en", false);
    let target_kfx = temp.path().join("ww_warn.kfx");

    // Pre-create a regular file where `.sdr` directory would be deployed so SdrDeployer::deploy fails
    // and populates `report.word_wise_error` while conversion itself still succeeds.
    std::fs::write(target_kfx.with_extension("sdr"), b"blocking-file").unwrap();

    let output = cli_cmd(temp.path())
        .arg(&epub_path)
        .arg("-o")
        .arg(&target_kfx)
        .output()
        .expect("Failed to execute binary");

    assert!(
        output.status.success(),
        "Non-fatal Word Wise failure must still exit 0; stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Warning: Word Wise generation failed:"),
        "Expected stderr warning, got: {stderr}"
    );
    assert!(
        !stdout.contains("Word Wise sidecar generated"),
        "Must NOT print Word Wise sidecar generated when word_wise_error is Some; stdout={stdout}"
    );
}
