use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;

fn bin() -> Command {
    Command::cargo_bin("scyros").unwrap()
}

#[test]
fn no_subcommand_fails_with_hint() {
    bin()
        .assert()
        .failure()
        .stderr(predicate::str::contains("subcommand"));
}

#[test]
fn version_flag_succeeds() {
    bin().arg("--version").assert().success();
}

#[test]
fn unknown_subcommand_fails() {
    bin().arg("notacommand").assert().failure();
}

#[test]
fn forks_missing_required_input_fails() {
    bin().arg("forks").assert().failure();
}

#[test]
fn duplicate_ids_missing_required_input_fails() {
    bin().arg("duplicate_ids").assert().failure();
}

#[test]
fn filter_languages_missing_required_input_fails() {
    bin().arg("filter_languages").assert().failure();
}

#[test]
fn forks_no_output() {
    bin()
        .args([
            "forks",
            "--input",
            "tests/data/phases/forks/forks.csv",
            "--no-output",
        ])
        .assert()
        .success();
}

#[test]
fn duplicate_ids_no_output() {
    bin()
        .args([
            "duplicate_ids",
            "--input",
            "tests/data/phases/duplicate_ids/duplicate_ids.csv",
            "--no-output",
        ])
        .assert()
        .success();
}

#[test]
fn filter_metadata_no_output() {
    bin()
        .args([
            "filter_metadata",
            "--input",
            "tests/data/phases/filter_metadata/filter_metadata.csv",
            "--no-output",
        ])
        .assert()
        .success();
}

#[test]
fn filter_languages_no_output() {
    bin()
        .args([
            "filter_languages",
            "--input",
            "tests/data/phases/filter_languages/filter_languages.csv",
            "--languages",
            "tests/data/keywords/scala_float.json",
            "--no-output",
        ])
        .assert()
        .success();
}

#[test]
fn forks_creates_output_file() {
    let output = "tests/data/cli_forks_out.csv";
    let _ = fs::remove_file(output);
    bin()
        .args([
            "forks",
            "--input",
            "tests/data/phases/forks/forks.csv",
            "--output",
            output,
        ])
        .assert()
        .success();
    assert!(std::path::Path::new(output).exists());
    fs::remove_file(output).unwrap();
}

// ── Error paths ──────────────────────────────────────────────────────────────

#[test]
fn forks_nonexistent_input_fails() {
    bin()
        .args(["forks", "--input", "nonexistent.csv"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("nonexistent.csv"));
}

#[test]
fn forks_output_exists_without_force_fails() {
    let output = "tests/data/cli_forks_no_force.csv";
    fs::write(output, b"").unwrap();
    bin()
        .args([
            "forks",
            "--input",
            "tests/data/phases/forks/forks.csv",
            "--output",
            output,
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--force"));
    fs::remove_file(output).unwrap();
}

#[test]
fn forks_force_overwrites_existing_output() {
    let output = "tests/data/cli_forks_force.csv";
    fs::write(output, b"").unwrap();
    bin()
        .args([
            "forks",
            "--input",
            "tests/data/phases/forks/forks.csv",
            "--output",
            output,
            "--force",
        ])
        .assert()
        .success();
    fs::remove_file(output).unwrap();
}

// ── Regression tests ────────────────────────────────────────────────────────

// --sub was defined without value_parser(usize), causing a panic instead of
// a clean parse when the flag was used.
#[test]
fn download_sub_flag_does_not_panic() {
    bin()
        .args([
            "download",
            "--input",
            "tests/data/phases/download/to_download_local_c.csv",
            "--dest",
            "target/tests/cli_sub_test",
            "--keywords",
            "tests/data/keywords/c.json",
            "--skip",
            "--count",
            "--sub",
            "1",
        ])
        .assert()
        .success();
}

#[test]
fn debug_flag_emits_error_chain() {
    let without_debug = bin()
        .args(["forks", "--input", "nonexistent.csv"])
        .output()
        .unwrap();
    let with_debug = bin()
        .args(["--debug", "forks", "--input", "nonexistent.csv"])
        .output()
        .unwrap();
    assert!(!without_debug.status.success());
    assert!(!with_debug.status.success());
    // --debug prints the full error chain ({:?}), which is longer than the plain message
    assert!(with_debug.stderr.len() >= without_debug.stderr.len());
}
