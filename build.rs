mod build_info_support;

fn git_output(root: &std::path::Path, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new("git")
        // Git hooks export repository-local variables. Discover this source root,
        // rather than accidentally identifying the hook's parent repository.
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout)
        .ok()
        .map(|text| text.trim().into())
}

fn watch_git_path(root: &std::path::Path, name: &str) {
    if let Some(path) = git_output(root, &["rev-parse", "--git-path", name]) {
        println!("cargo:rerun-if-changed={}", root.join(path).display());
    }
}

fn builder_value(key: &str) -> Option<String> {
    std::env::var_os(key).map(|value| value.into_string().expect("builder metadata must be UTF-8"))
}

fn main() {
    // Watch source changes and external worktree refs/index. Paths are Cargo
    // invalidation inputs only; none are embedded in the executable metadata.
    println!("cargo:rerun-if-changed=.");
    for key in ["APERTURE_BUILD_REVISION", "APERTURE_BUILD_SOURCE_STATE"] {
        println!("cargo:rerun-if-env-changed={key}");
    }
    let root = std::path::PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").expect("Cargo supplies the manifest directory"),
    );
    for name in ["HEAD", "index", "packed-refs"] {
        watch_git_path(&root, name);
    }
    if let Some(reference) = git_output(&root, &["rev-parse", "--symbolic-full-name", "HEAD"]) {
        watch_git_path(&root, &reference);
    }
    let revision = builder_value("APERTURE_BUILD_REVISION");
    let state = builder_value("APERTURE_BUILD_SOURCE_STATE");
    let (revision, state) =
        build_info_support::override_info(revision.as_deref(), state.as_deref())
            .expect("invalid builder metadata")
            .unwrap_or_else(|| build_info_support::git_info(&root));
    println!("cargo:rustc-env=APERTURE_SOURCE_REVISION={revision}");
    println!("cargo:rustc-env=APERTURE_SOURCE_STATE={state}");
}
