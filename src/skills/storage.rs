use super::{
    invalid, metadata,
    source::{self, Import},
};
use crate::{atomic::DirLock, error::Error};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::{Component, Path, PathBuf},
};

pub(super) const PROVENANCE: &str = ".aperture-skill.json";
/// Installer ownership is explicit; manually placed skills are read-only.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Provenance {
    pub format: u8,
    pub name: String,
    pub source_type: String,
    pub source_identity: String,
    pub content_sha256: String,
}

pub(super) fn resolve_root(config: &Path, directory: &str) -> Result<PathBuf, Error> {
    if directory.trim().is_empty() || directory.starts_with('~') || directory.contains('\0') {
        return Err(invalid(
            "skills.directory must be a nonempty literal path; tilde expansion is unsupported",
        ));
    }
    let path = Path::new(directory);
    let root = if path.is_absolute() {
        path.to_path_buf()
    } else {
        config.join(path)
    };
    if root
        .components()
        .any(|part| matches!(part, Component::ParentDir))
    {
        return Err(invalid("skills.directory cannot contain parent traversal"));
    }
    pin_root(&root)
}

/// Pin the explicitly configured trust boundary without creating it. Existing
/// root aliases are trusted; dangling aliases fail instead of becoming directories.
fn pin_root(path: &Path) -> Result<PathBuf, Error> {
    match path.symlink_metadata() {
        Ok(_) => Ok(path.canonicalize()?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => pin_missing_root(path),
        Err(error) => Err(error.into()),
    }
}

fn pin_missing_root(path: &Path) -> Result<PathBuf, Error> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .ok_or_else(|| invalid("Invalid library root"))?;
    Ok(pin_root(parent)?.join(name))
}

/// Reject symlinks in every existing ancestor, not just the final entry.
/// Advisory locking assumes other writers cooperate; hostile concurrent filesystem
/// mutation by another local process is outside this instruction-library boundary.
pub(super) fn check_path(path: &Path) -> Result<(), Error> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component);
        match current.symlink_metadata() {
            Ok(metadata) if metadata.is_symlink() => {
                return Err(invalid("Symlinked skill paths are forbidden"))
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}
pub(super) fn safe_relative(path: &Path) -> Result<(), Error> {
    for component in path.components() {
        let Component::Normal(name) = component else {
            return Err(invalid("Unsafe skill reference path"));
        };
        let name = name
            .to_str()
            .ok_or_else(|| invalid("Skill reference paths must be UTF-8"))?;
        safe_filename(name)?;
    }
    Ok(())
}
fn safe_filename(name: &str) -> Result<(), Error> {
    if name.is_empty()
        || name.len() > 255
        || name.ends_with(['.', ' '])
        || name
            .chars()
            .any(|c| c.is_control() || "<>:\"/\\|?*".contains(c))
    {
        return Err(invalid("Unsafe skill reference filename"));
    }
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end_matches(' ')
        .to_ascii_lowercase();
    if metadata::reserved(&stem) {
        return Err(invalid("Skill reference filename has a reserved stem"));
    }
    Ok(())
}
fn lock(root: &Path) -> Result<DirLock, Error> {
    check_path(root)?;
    let path = root.join(".aperture.lock");
    check_path(&path)?;
    if path.exists() {
        regular_file(&path)?;
    }
    Ok(DirLock::acquire(root)?)
}

pub(super) fn install(
    root: &Path,
    imported: &Import,
    override_name: Option<&str>,
    replace: bool,
) -> Result<String, Error> {
    let name = import_name(imported, override_name)?;
    validate_files(&imported.files)?;
    let provenance = import_provenance(imported, &name);
    let _lock = lock(root)?;
    let target = root.join(&name);
    prepare_target(root, &target, &name, replace)?;
    publish_import(root, &target, &imported.files, &provenance)?;
    Ok(name)
}
fn publish_import(
    root: &Path,
    target: &Path,
    files: &BTreeMap<PathBuf, Vec<u8>>,
    provenance: &Provenance,
) -> Result<(), Error> {
    let stage = create_stage(root)?;
    let result = stage_files(&stage, files, provenance).and_then(|()| commit(&stage, target));
    if stage.exists() {
        std::fs::remove_dir_all(&stage)?;
    }
    result
}
fn validate_files(files: &BTreeMap<PathBuf, Vec<u8>>) -> Result<(), Error> {
    let mut folded = BTreeMap::new();
    for path in files.keys() {
        validate_import_path(path, &mut folded)?;
    }
    Ok(())
}
fn validate_import_path(path: &Path, folded: &mut BTreeMap<String, PathBuf>) -> Result<(), Error> {
    if path.components().any(|part| {
        part.as_os_str()
            .to_string_lossy()
            .eq_ignore_ascii_case(PROVENANCE)
    }) {
        return Err(invalid("Source cannot supply installer metadata"));
    }
    safe_relative(path)?;
    for ancestor in path.ancestors().filter(|path| !path.as_os_str().is_empty()) {
        let key = ancestor.to_string_lossy().to_ascii_lowercase();
        if folded
            .get(&key)
            .is_some_and(|previous| previous != ancestor)
        {
            return Err(invalid("Case-insensitive reference collision"));
        }
        folded.insert(key, ancestor.to_path_buf());
    }
    Ok(())
}
fn check_collision(root: &Path, name: &str) -> Result<(), Error> {
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        if entry
            .file_name()
            .to_string_lossy()
            .eq_ignore_ascii_case(name)
            && entry.file_name() != name
        {
            return Err(invalid("Case-insensitive skill identifier collision"));
        }
    }
    Ok(())
}
fn create_stage(root: &Path) -> Result<PathBuf, Error> {
    for _ in 0..16 {
        let stage = root.join(format!(".skill-stage-{:016x}", fastrand::u64(..)));
        match std::fs::create_dir(&stage) {
            Ok(()) => return Ok(stage),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
    }
    Err(invalid("Could not allocate skill staging directory"))
}
fn stage_files(
    stage: &Path,
    files: &BTreeMap<PathBuf, Vec<u8>>,
    provenance: &Provenance,
) -> Result<(), Error> {
    for (relative, bytes) in files {
        let destination = stage.join(relative);
        check_path(&destination)?;
        std::fs::create_dir_all(
            destination
                .parent()
                .ok_or_else(|| invalid("Invalid skill destination"))?,
        )?;
        write_new(&destination, bytes)?;
    }
    write_new(
        &stage.join(PROVENANCE),
        &serde_json::to_vec_pretty(provenance)?,
    )?;
    Ok(())
}
fn write_new(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    use std::io::Write;
    check_path(path)?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}
fn commit(stage: &Path, target: &Path) -> Result<(), Error> {
    if !target.exists() {
        return Ok(std::fs::rename(stage, target)?);
    }
    let parent = target
        .parent()
        .ok_or_else(|| invalid("Invalid skill destination"))?;
    let backup = create_stage(parent)?;
    // Rename into an empty container, so backup allocation never overwrites data.
    let old = backup.join("old");
    move_original(target, &old, &backup)?;
    replace_or_restore(stage, target, &old, &backup)?;
    std::fs::remove_dir_all(&backup)?;
    Ok(())
}
pub(super) fn owned(target: &Path, name: &str) -> Result<Provenance, Error> {
    let bytes = ownership_bytes(target)?;
    let provenance: Provenance =
        serde_json::from_slice(&bytes).map_err(|_| invalid("Invalid installer metadata"))?;
    validate_owner(&provenance, name)?;
    Ok(provenance)
}
fn validate_provenance(provenance: &Provenance) -> Result<(), Error> {
    if !matches!(
        provenance.source_type.as_str(),
        "stdin" | "markdown" | "file" | "directory" | "url"
    ) {
        return Err(invalid("Invalid installer source type"));
    }
    if provenance.content_sha256.len() != 64
        || !provenance
            .content_sha256
            .bytes()
            .all(|b| b.is_ascii_hexdigit())
    {
        return Err(invalid("Invalid installer content hash"));
    }
    if provenance.source_type == "url" {
        validate_url_identity(&provenance.source_identity)?;
    }
    Ok(())
}
pub(super) fn hash(files: &BTreeMap<PathBuf, Vec<u8>>) -> String {
    let mut digest = Sha256::new();
    for (path, bytes) in files {
        let name = path.to_string_lossy();
        digest.update((name.len() as u64).to_le_bytes());
        digest.update(name.as_bytes());
        digest.update((bytes.len() as u64).to_le_bytes());
        digest.update(bytes);
    }
    format!("{:x}", digest.finalize())
}
pub(super) fn uninstall(root: &Path, name: &str) -> Result<(), Error> {
    metadata::user_name(name)?;
    let _lock = lock(root)?;
    let target = root.join(name);
    owned(&target, name)?;
    source::snapshot(&target)?;
    let trash = create_stage(root)?;
    if let Err(error) = std::fs::rename(&target, trash.join("old")) {
        std::fs::remove_dir(&trash)?;
        return Err(error.into());
    }
    std::fs::remove_dir_all(trash)?;
    Ok(())
}

pub(super) fn read_lock(root: &Path) -> Result<Option<DirLock>, Error> {
    if !root.exists() {
        check_path(root)?;
        return Ok(None);
    }
    lock(root).map(Some)
}

fn import_name(imported: &Import, override_name: Option<&str>) -> Result<String, Error> {
    let content = imported
        .files
        .get(Path::new("SKILL.md"))
        .ok_or_else(|| invalid("Directory skill requires SKILL.md"))?;
    let parsed = metadata::parse(source::markdown(content)?)?;
    let name = override_name.or(parsed.name.as_deref()).or(imported.fallback.as_deref()).ok_or_else(|| invalid("Skill requires frontmatter name or --name (files/directories may use their basename)"))?.to_string();
    metadata::user_name(&name)?;
    Ok(name)
}
fn import_provenance(imported: &Import, name: &str) -> Provenance {
    Provenance {
        format: 1,
        name: name.into(),
        source_type: imported.source_type.clone(),
        source_identity: imported.identity.clone(),
        content_sha256: hash(&imported.files),
    }
}
fn prepare_target(root: &Path, target: &Path, name: &str, replace: bool) -> Result<(), Error> {
    check_path(target)?;
    check_collision(root, name)?;
    if !target.exists() {
        return Ok(());
    }
    if !replace {
        return Err(invalid("Skill already exists; use --replace explicitly"));
    }
    owned(target, name)?;
    source::snapshot(target)?;

    Ok(())
}

fn replace_or_restore(stage: &Path, target: &Path, old: &Path, backup: &Path) -> Result<(), Error> {
    let Err(error) = std::fs::rename(stage, target) else {
        return Ok(());
    };
    if std::fs::rename(old, target).is_err() {
        return Err(invalid(format!(
            "Replacement failed; original retained at {}",
            old.display()
        )));
    }
    std::fs::remove_dir(backup)?;
    Err(error.into())
}

fn validate_owner(provenance: &Provenance, name: &str) -> Result<(), Error> {
    if provenance.format != 1 || provenance.name != name {
        return Err(invalid(
            "Installer metadata does not match skill identifier",
        ));
    }
    validate_provenance(provenance)
}

fn validate_url_identity(identity: &str) -> Result<(), Error> {
    let url =
        reqwest::Url::parse(identity).map_err(|_| invalid("Invalid installer URL identity"))?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(invalid("Unsafe installer URL identity"));
    }
    Ok(())
}

fn move_original(target: &Path, old: &Path, backup: &Path) -> Result<(), Error> {
    if let Err(error) = std::fs::rename(target, old) {
        std::fs::remove_dir(backup)?;
        return Err(error.into());
    }
    Ok(())
}

fn regular_file(path: &Path) -> Result<(), Error> {
    if !path.symlink_metadata()?.is_file() {
        return Err(invalid("Installer metadata and lock must be regular files"));
    }
    Ok(())
}

fn ownership_bytes(target: &Path) -> Result<Vec<u8>, Error> {
    check_path(target)?;
    let path = target.join(PROVENANCE);
    check_path(&path)?;
    regular_file(&path).map_err(|_| invalid("Manually placed or corrupt skills cannot be replaced/uninstalled; installer ownership metadata required"))?;
    source::bounded_read(std::fs::File::open(path)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacement_rename_failure_restores_original() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("release");
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("SKILL.md"), "original").unwrap();
        // A nonexistent staging path forces the publish rename to fail after backup.
        assert!(commit(&root.path().join("missing-stage"), &target).is_err());
        assert_eq!(
            std::fs::read_to_string(target.join("SKILL.md")).unwrap(),
            "original"
        );
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
    }

    #[test]
    fn stage_failure_leaves_existing_target_untouched() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("release");
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("SKILL.md"), "original").unwrap();
        let provenance = Provenance {
            format: 1,
            name: "release".into(),
            source_type: "markdown".into(),
            source_identity: "literal Markdown".into(),
            content_sha256: "0".repeat(64),
        };
        let files = BTreeMap::from([(PathBuf::from("SKILL.md"), b"new".to_vec())]);
        let blocked_stage = root.path().join("blocked-stage");
        std::fs::write(&blocked_stage, "blocker").unwrap();
        assert!(stage_files(&blocked_stage, &files, &provenance).is_err());
        assert_eq!(
            std::fs::read_to_string(target.join("SKILL.md")).unwrap(),
            "original"
        );
    }
}
