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

#[test]
fn discovery_commands_help_describes_canonical_roles() {
    aperture_cmd()
        .args(["commands", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Canonical role: quick structure lookup",
        ));

    aperture_cmd()
        .args(["search", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Canonical role: primary human discovery entry point",
        ));

    aperture_cmd()
        .args(["docs", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Canonical role: inspect exact operation details",
        ));

    aperture_cmd()
        .args(["overview", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Canonical role: high-level orientation",
        ));
}
