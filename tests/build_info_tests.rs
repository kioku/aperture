#[path = "../build_info_support.rs"]
mod support;

#[test]
fn validates_builder_metadata() {
    let revision = "ce7082d0003cb0b35ae9ed19c5b9a79b1dc75a6f";
    assert_eq!(
        support::override_info(Some(revision), Some("dirty")).unwrap(),
        Some((revision.into(), "dirty".into()))
    );
    assert_eq!(
        support::override_info(Some("unknown"), Some("unknown")).unwrap(),
        Some(("unknown".into(), "unknown".into()))
    );
    for bad in ["main", "abc", "../secret", "abc\n"] {
        assert!(support::override_info(Some(bad), Some("clean")).is_err());
    }
    assert!(support::override_info(Some(revision), Some("yes")).is_err());
    assert!(support::override_info(None, Some("clean")).is_err());
    assert!(support::override_info(Some("unknown"), Some("clean")).is_err());
}

#[test]
fn detects_git_and_unavailable_metadata() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(
        support::git_info(dir.path()),
        ("unknown".into(), "unknown".into())
    );
    let git = |args: &[&str]| {
        assert!(std::process::Command::new("git")
            .args(args)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .env_remove("GIT_COMMON_DIR")
            .current_dir(dir.path())
            .status()
            .unwrap()
            .success());
    };
    git(&["init", "--quiet"]);
    git(&[
        "-c",
        "user.name=Test",
        "-c",
        "user.email=test@example.invalid",
        "commit",
        "--quiet",
        "--allow-empty",
        "-m",
        "test",
    ]);
    let (revision, state) = support::git_info(dir.path());
    assert_eq!(revision.len(), 40);
    assert_eq!(state, "clean");
    let archive = dir.path().join("archive");
    std::fs::create_dir(&archive).unwrap();
    assert_eq!(
        support::git_info(&archive),
        ("unknown".into(), "unknown".into())
    );
    std::fs::write(dir.path().join("untracked"), "dirty").unwrap();
    assert_eq!(support::git_info(dir.path()), (revision, "dirty".into()));
}

#[test]
fn cli_build_info_and_version() {
    use aperture_cli::cli::{Cli, Commands};
    use clap::Parser;
    let version = Cli::try_parse_from(["aperture", "--version"]).unwrap_err();
    assert_eq!(version.kind(), clap::error::ErrorKind::DisplayVersion);
    assert_eq!(
        version.to_string(),
        format!("aperture-cli {}\n", env!("CARGO_PKG_VERSION"))
    );
    for json in [false, true] {
        let mut args = vec!["aperture", "build-info"];
        if json {
            args.push("--json");
        }
        let cli = Cli::try_parse_from(args).unwrap();
        assert!(matches!(cli.command, Commands::BuildInfo { json: parsed } if parsed == json));
    }
    let info = aperture_cli::build_info::current();
    let value = serde_json::to_value(info).unwrap();
    assert_eq!(value["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(value["revision"], env!("APERTURE_SOURCE_REVISION"));
    assert_eq!(value["source_state"], env!("APERTURE_SOURCE_STATE"));
}

#[test]
fn build_script_handles_missing_git_and_overrides() {
    let dir = tempfile::tempdir().unwrap();
    let helper = dir
        .path()
        .join(format!("build-info-helper{}", std::env::consts::EXE_SUFFIX));
    assert!(std::process::Command::new("rustc")
        .args(["--edition=2021", "build.rs", "-o"])
        .arg(&helper)
        .status()
        .unwrap()
        .success());
    let run = |revision: Option<&str>, state: Option<&str>| {
        let mut command = std::process::Command::new(&helper);
        command
            .env("CARGO_MANIFEST_DIR", dir.path())
            .env("PATH", dir.path())
            .env_remove("APERTURE_BUILD_REVISION")
            .env_remove("APERTURE_BUILD_SOURCE_STATE");
        if let Some(revision) = revision {
            command.env("APERTURE_BUILD_REVISION", revision);
        }
        if let Some(state) = state {
            command.env("APERTURE_BUILD_SOURCE_STATE", state);
        }
        command.output().unwrap()
    };
    let unknown = run(None, None);
    assert!(unknown.status.success());
    assert!(String::from_utf8(unknown.stdout)
        .unwrap()
        .contains("APERTURE_SOURCE_REVISION=unknown"));
    let first = run(
        Some("ce7082d0003cb0b35ae9ed19c5b9a79b1dc75a6f"),
        Some("clean"),
    );
    let second = run(
        Some("1111111111111111111111111111111111111111"),
        Some("dirty"),
    );
    assert!(first.status.success());
    assert!(second.status.success());
    assert_ne!(first.stdout, second.stdout);
    let supplied = String::from_utf8(second.stdout).unwrap();
    assert!(supplied.contains("APERTURE_SOURCE_REVISION=1111111111111111111111111111111111111111"));
    assert!(supplied.contains("APERTURE_SOURCE_STATE=dirty"));
    assert!(!run(Some("main"), Some("clean")).status.success());
}

#[test]
fn binary_reports_identity_without_configuration() {
    let dir = tempfile::tempdir().unwrap();
    // A file cannot be used as a config directory: metadata must bypass it.
    let config = dir.path().join("not-a-directory");
    std::fs::write(&config, "invalid config location").unwrap();
    let run = |args: &[&str]| {
        std::process::Command::new(env!("CARGO_BIN_EXE_aperture"))
            .args(args)
            .env("APERTURE_CONFIG_DIR", &config)
            .output()
            .unwrap()
    };
    let json = run(&["build-info", "--json"]);
    assert!(json.status.success());
    let value: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(value["revision"], env!("APERTURE_SOURCE_REVISION"));
    assert_eq!(value["source_state"], env!("APERTURE_SOURCE_STATE"));
    assert_eq!(value["version"], env!("CARGO_PKG_VERSION"));
    let text = run(&["build-info"]);
    assert!(text.status.success());
    assert!(String::from_utf8(text.stdout)
        .unwrap()
        .contains("Source revision:"));
    let version = run(&["--version"]);
    assert!(version.status.success());
    assert_eq!(
        String::from_utf8(version.stdout).unwrap(),
        format!("aperture-cli {}\n", env!("CARGO_PKG_VERSION"))
    );
}
