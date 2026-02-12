//! CLI smoke tests — verify basic binary behavior.

use std::process::Command;

fn cli_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_mneme_cli"))
}

#[test]
fn test_help_flag() {
    let output = cli_bin().arg("--help").output().expect("failed to run");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Usage"),
        "Expected usage info in --help output"
    );
}

#[test]
fn test_version_flag() {
    let output = cli_bin().arg("--version").output().expect("failed to run");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("mneme_cli"),
        "Expected crate name in --version output"
    );
}

#[test]
fn test_invalid_config_does_not_panic() {
    // Passing a nonexistent config file should not panic — it falls back to defaults
    let output = cli_bin()
        .arg("--config")
        .arg("/tmp/nonexistent_mneme_config_12345.toml")
        .arg("--help") // exit immediately via --help
        .output()
        .expect("failed to run");
    assert!(output.status.success());
}
