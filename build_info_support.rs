use std::path::Path;
use std::process::Command;

/// Overrides are a complete pair. Only full Git object IDs (SHA-1 or SHA-256)
/// are accepted; unknown revisions cannot claim a clean or dirty checkout.
pub fn override_info(
    revision: Option<&str>,
    state: Option<&str>,
) -> Result<Option<(String, String)>, &'static str> {
    match (revision, state) {
        (None, None) => Ok(None),
        (Some(revision), Some(state)) => {
            if !valid_revision(revision) || !valid_state(revision, state) {
                return Err("invalid APERTURE_BUILD_REVISION/APERTURE_BUILD_SOURCE_STATE");
            }
            Ok(Some((revision.to_ascii_lowercase(), state.into())))
        }
        _ => {
            Err("APERTURE_BUILD_REVISION and APERTURE_BUILD_SOURCE_STATE must be supplied together")
        }
    }
}

fn git_output(root: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
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
    output
        .status
        .success()
        .then(|| String::from_utf8(output.stdout).ok())
        .flatten()
}

/// Untracked files also make a source snapshot dirty. Failures never imply clean.
pub fn git_info(root: &Path) -> (String, String) {
    // An archive unpacked inside another repository must not borrow its HEAD.
    let top = git_output(root, &["rev-parse", "--show-toplevel"]);
    if top
        .as_deref()
        .and_then(|path| Path::new(path.trim()).canonicalize().ok())
        != root.canonicalize().ok()
    {
        return ("unknown".into(), "unknown".into());
    }
    let Some(revision) = git_output(root, &["rev-parse", "--verify", "HEAD"]) else {
        return ("unknown".into(), "unknown".into());
    };
    let state = git_output(root, &["status", "--porcelain", "--untracked-files=normal"]).map_or(
        "unknown",
        |status| if status.is_empty() { "clean" } else { "dirty" },
    );
    override_info(Some(revision.trim()), Some(state))
        .ok()
        .flatten()
        .unwrap_or_else(|| ("unknown".into(), "unknown".into()))
}

fn valid_revision(revision: &str) -> bool {
    revision == "unknown"
        || ([40, 64].contains(&revision.len())
            && revision.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

fn valid_state(revision: &str, state: &str) -> bool {
    ["clean", "dirty", "unknown"].contains(&state) && (revision != "unknown" || state == "unknown")
}
