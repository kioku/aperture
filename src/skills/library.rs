use super::{invalid, metadata, source, storage};
use crate::error::Error;
use base64::Engine;
use serde::Serialize;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

const CORE: &str = include_str!("core.md");

#[derive(Serialize)]
pub(super) struct Entry {
    pub name: String,
    pub description: String,
    pub origin: &'static str,
    pub required_apis: Vec<String>,
    pub configured_apis: Vec<String>,
    pub missing_apis: Vec<String>,
    pub readiness: &'static str,
    pub package_revision: Option<&'static str>,
    pub provenance: Option<storage::Provenance>,
    pub content: Option<String>,
    /// Reference bytes are UTF-8 when possible, otherwise explicitly base64 encoded.
    pub files: BTreeMap<String, FileContent>,
}
#[derive(Serialize)]
pub(super) struct FileContent {
    encoding: &'static str,
    content: String,
}
impl std::fmt::Display for FileContent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "({}) {}", self.encoding, self.content)
    }
}

pub(super) fn discover(
    root: &Path,
    selected: Option<&str>,
    full: bool,
    get: bool,
    apis: &[String],
) -> Result<Vec<Entry>, Error> {
    if let Some(name) = selected {
        metadata::validate_name(name)?;
    }
    let _lock = storage::read_lock(root)?;
    let mut entries = Vec::new();
    append_core(&mut entries, selected, apis, get)?;
    append_installed(&mut entries, root, selected, full, get, apis)?;
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(entries)
}
fn names(root: &Path) -> Result<Vec<String>, Error> {
    let mut names = Vec::new();
    for child in std::fs::read_dir(root)? {
        let child = child?;
        let name = child
            .file_name()
            .into_string()
            .map_err(|_| invalid("Non-UTF-8 skill identifier in library"))?;
        if name.starts_with('.') {
            continue;
        }
        metadata::user_name(&name)?;
        validate_directory(&child)?;
        names.push(name);
    }
    names.sort();
    Ok(names)
}
fn installed(
    root: &Path,
    name: &str,
    full: bool,
    get: bool,
    apis: &[String],
) -> Result<Entry, Error> {
    let target = installed_target(root, name)?;
    let mut files = source::snapshot(&target)?;
    let content = files
        .remove(Path::new("SKILL.md"))
        .ok_or_else(|| invalid(format!("Skill {name} is missing SKILL.md")))?;
    let mut entry = entry(name, source::markdown(&content)?, apis, get)?;
    entry.provenance = read_provenance(&target, name)?;
    files.remove(Path::new(storage::PROVENANCE));
    entry.files = selected_files(files, full)?;
    Ok(entry)
}
fn entry(name: &str, content: &str, apis: &[String], get: bool) -> Result<Entry, Error> {
    let metadata = metadata::parse(content)?;
    let mut required_apis = metadata.aperture.required_apis.clone();
    required_apis.sort();
    let (configured_apis, missing_apis) = required_apis
        .iter()
        .cloned()
        .partition(|api| apis.contains(api));
    Ok(Entry {
        name: name.into(),
        description: metadata::description(&metadata, content),
        origin: "installed",
        required_apis,
        configured_apis,
        missing_apis,
        readiness: "local_configuration_only",
        package_revision: None,
        provenance: None,
        content: get.then(|| content.into()),
        files: BTreeMap::new(),
    })
}
fn full_files(files: BTreeMap<PathBuf, Vec<u8>>) -> Result<BTreeMap<String, FileContent>, Error> {
    files
        .into_iter()
        .map(|(path, bytes)| {
            let path = path
                .to_str()
                .ok_or_else(|| invalid("Non-UTF-8 skill reference path"))?
                .replace('\\', "/");
            let file = match String::from_utf8(bytes) {
                Ok(content) => FileContent {
                    encoding: "utf-8",
                    content,
                },
                Err(error) => FileContent {
                    encoding: "base64",
                    content: base64::engine::general_purpose::STANDARD.encode(error.as_bytes()),
                },
            };
            Ok((path, file))
        })
        .collect()
}

fn append_core(
    entries: &mut Vec<Entry>,
    selected: Option<&str>,
    apis: &[String],
    get: bool,
) -> Result<(), Error> {
    if selected.is_none() || selected == Some("core") {
        let mut core = entry("core", CORE, apis, get)?;
        core.origin = "bundled";
        core.package_revision = Some(crate::build_info::current().revision);
        entries.push(core);
    }
    Ok(())
}
fn append_installed(
    entries: &mut Vec<Entry>,
    root: &Path,
    selected: Option<&str>,
    full: bool,
    get: bool,
    apis: &[String],
) -> Result<(), Error> {
    if let Some(name) = selected.filter(|name| *name != "core") {
        entries.push(installed(root, name, full, get, apis)?);
    } else if selected.is_none() && root.exists() {
        append_library(entries, root, full, get, apis)?;
    }
    Ok(())
}
fn append_library(
    entries: &mut Vec<Entry>,
    root: &Path,
    full: bool,
    get: bool,
    apis: &[String],
) -> Result<(), Error> {
    for name in names(root)? {
        entries.push(installed(root, &name, full, get, apis)?);
    }
    Ok(())
}

fn read_provenance(target: &Path, name: &str) -> Result<Option<storage::Provenance>, Error> {
    if target.join(storage::PROVENANCE).symlink_metadata().is_ok() {
        return storage::owned(target, name).map(Some);
    }
    Ok(None)
}

fn validate_directory(child: &std::fs::DirEntry) -> Result<(), Error> {
    if !child.file_type()?.is_dir() {
        return Err(invalid("Unexpected non-directory entry in skill library"));
    }
    Ok(())
}

fn installed_target(root: &Path, name: &str) -> Result<PathBuf, Error> {
    metadata::user_name(name)?;
    Ok(root.join(name))
}

fn selected_files(
    files: BTreeMap<PathBuf, Vec<u8>>,
    full: bool,
) -> Result<BTreeMap<String, FileContent>, Error> {
    if full {
        full_files(files)
    } else {
        Ok(BTreeMap::new())
    }
}
