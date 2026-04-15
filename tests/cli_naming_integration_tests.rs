mod common;

use common::aperture_cmd;
use predicates::prelude::*;

#[test]
fn commands_canonical_name_renders_help() {
    aperture_cmd()
        .args(["commands", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Legacy alias: list-commands"))
        .stdout(predicate::str::contains("aperture commands myapi"));
}

#[test]
fn commands_legacy_alias_still_renders_help() {
    aperture_cmd()
        .args(["list-commands", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Legacy alias: list-commands"))
        .stdout(predicate::str::contains("aperture commands myapi"));
}

#[test]
fn run_canonical_name_renders_help() {
    aperture_cmd()
        .args(["run", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Legacy alias: exec"))
        .stdout(predicate::str::contains(
            "aperture run getUserById --id 123",
        ));
}

#[test]
fn exec_legacy_alias_still_renders_help() {
    aperture_cmd()
        .args(["exec", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Legacy alias: exec"))
        .stdout(predicate::str::contains(
            "aperture run getUserById --id 123",
        ));
}
