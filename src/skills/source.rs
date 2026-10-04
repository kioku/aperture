use super::{invalid, storage};
use crate::error::Error;
use std::{
    collections::BTreeMap,
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};

pub(super) const MAX_FILE: usize = 1024 * 1024;
pub(super) const MAX_TOTAL: usize = 8 * MAX_FILE;
pub(super) const MAX_FILES: usize = 128;
pub(super) const MAX_DEPTH: usize = 16;

/// A fully validated snapshot, not a source path followed during installation.
pub(super) struct Import {
    pub files: BTreeMap<PathBuf, Vec<u8>>,
    pub source_type: String,
    pub identity: String,
    pub fallback: Option<String>,
}

pub(super) async fn load(source: &str) -> Result<Import, Error> {
    if source == "-" {
        let bytes = bounded_read(std::io::stdin().lock())?;
        return single(bytes, "stdin", "stdin".into(), None);
    }
    if url_looking(source) {
        return download(source).await;
    }
    let path = Path::new(source);
    if path.symlink_metadata().is_ok() {
        return local(path);
    }
    if path_looking(source) {
        return Err(invalid("Local skill source does not exist"));
    }
    single(
        source.as_bytes().to_vec(),
        "markdown",
        "literal Markdown".into(),
        None,
    )
}
fn path_looking(source: &str) -> bool {
    if literal_looking(source) {
        return false;
    }
    source.contains('/')
        || source.contains('\\')
        || source.starts_with('.')
        || source.starts_with('~')
        || Path::new(source)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
        || source.contains(':')
}

fn single(
    bytes: Vec<u8>,
    kind: &str,
    identity: String,
    fallback: Option<String>,
) -> Result<Import, Error> {
    markdown(&bytes)?;
    Ok(Import {
        files: BTreeMap::from([(PathBuf::from("SKILL.md"), bytes)]),
        source_type: kind.into(),
        identity,
        fallback,
    })
}
pub(super) fn markdown(bytes: &[u8]) -> Result<&str, Error> {
    if bytes.len() > MAX_FILE {
        return Err(invalid("Skill file exceeds 1 MiB limit"));
    }
    std::str::from_utf8(bytes).map_err(|_| invalid("Skill Markdown must be UTF-8"))
}
pub(super) fn bounded_read(reader: impl Read) -> Result<Vec<u8>, Error> {
    let mut bytes = Vec::new();
    reader.take((MAX_FILE + 1) as u64).read_to_end(&mut bytes)?;
    if bytes.len() > MAX_FILE {
        return Err(invalid("Skill file exceeds 1 MiB limit"));
    }
    Ok(bytes)
}
fn local(path: &Path) -> Result<Import, Error> {
    storage::check_path(path)?;
    let metadata = path.symlink_metadata()?;
    let identity = path.canonicalize()?.to_string_lossy().into_owned();
    let fallback = path
        .file_stem()
        .and_then(|name| name.to_str())
        .map(str::to_owned);
    if metadata.is_file() {
        return single(
            bounded_read(std::fs::File::open(path)?)?,
            "file",
            identity,
            fallback,
        );
    }
    if !metadata.is_dir() {
        return Err(invalid("Skill source must be a regular file or directory"));
    }
    directory_import(path, identity)
}
fn directory_import(path: &Path, identity: String) -> Result<Import, Error> {
    Ok(Import {
        files: snapshot(path)?,
        source_type: "directory".into(),
        identity,
        fallback: path
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_owned),
    })
}

pub(super) fn snapshot(root: &Path) -> Result<BTreeMap<PathBuf, Vec<u8>>, Error> {
    storage::check_path(root)?;
    let mut files = BTreeMap::new();
    collect(root, root, 0, &mut files, &mut 0)?;
    Ok(files)
}
fn collect(
    root: &Path,
    directory: &Path,
    depth: usize,
    files: &mut BTreeMap<PathBuf, Vec<u8>>,
    entries: &mut usize,
) -> Result<(), Error> {
    if depth > MAX_DEPTH {
        return Err(invalid("Skill directory exceeds depth limit (16)"));
    }
    let mut children = std::fs::read_dir(directory)?
        .take(MAX_FILES + 2)
        .collect::<Result<Vec<_>, _>>()?;
    children.sort_by_key(std::fs::DirEntry::file_name);
    *entries += children
        .iter()
        .filter(|entry| entry.file_name() != storage::PROVENANCE)
        .count();
    if *entries > MAX_FILES {
        return Err(invalid("Skill directory exceeds entry limit (128)"));
    }
    for child in children {
        collect_entry(root, &child.path(), depth, files, entries)?;
    }
    Ok(())
}
fn collect_entry(
    root: &Path,
    path: &Path,
    depth: usize,
    files: &mut BTreeMap<PathBuf, Vec<u8>>,
    entries: &mut usize,
) -> Result<(), Error> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| invalid("Unsafe skill path"))?;
    storage::safe_relative(relative)?;
    let metadata = path.symlink_metadata()?;
    if metadata.is_symlink() {
        return Err(invalid("Symlinks are not permitted in skills"));
    }
    if metadata.is_dir() {
        return collect(root, path, depth + 1, files, entries);
    }
    if !metadata.is_file() {
        return Err(invalid("Only regular skill files are permitted"));
    }
    collect_file(path, relative, files)
}
fn collect_file(
    path: &Path,
    relative: &Path,
    files: &mut BTreeMap<PathBuf, Vec<u8>>,
) -> Result<(), Error> {
    let bytes = bounded_read(std::fs::File::open(path)?)?;
    let content_size: usize = files
        .iter()
        .filter(|(path, _)| path.as_path() != Path::new(storage::PROVENANCE))
        .map(|(_, bytes)| bytes.len())
        .sum();
    if relative != Path::new(storage::PROVENANCE) && content_size + bytes.len() > MAX_TOTAL {
        return Err(invalid("Skill exceeds 8 MiB total limit"));
    }
    files.insert(relative.to_path_buf(), bytes);
    Ok(())
}

fn safe_url(source: &str) -> Result<reqwest::Url, Error> {
    let url = reqwest::Url::parse(source).map_err(|_| invalid("Invalid skill URL"))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(invalid("Skill URLs must use HTTP(S)"));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(invalid("Credential-bearing skill URLs are forbidden"));
    }
    Ok(url)
}
async fn download(source: &str) -> Result<Import, Error> {
    let mut url = safe_url(source)?;
    let mut identity = url.clone();
    identity.set_query(None);
    identity.set_fragment(None);
    let identity = identity.to_string();
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|_| invalid("Could not create skill download client"))?;
    let download = fetch(&client, &mut url, &identity);
    let bytes = tokio::time::timeout(Duration::from_secs(20), download)
        .await
        .map_err(|_| invalid("Skill download exceeded 20 second deadline"))??;
    single(bytes, "url", identity, None)
}
fn redirect_url(url: &reqwest::Url, response: &reqwest::Response) -> Result<reqwest::Url, Error> {
    let location = response
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| invalid("Invalid skill URL redirect"))?;
    let next = url
        .join(location)
        .map_err(|_| invalid("Invalid skill URL redirect"))?;
    safe_url(next.as_str())
}
async fn read_response(mut response: reqwest::Response, identity: &str) -> Result<Vec<u8>, Error> {
    if !response.status().is_success() {
        return Err(invalid(format!(
            "Skill download HTTP status {}: {identity}",
            response.status().as_u16()
        )));
    }
    if response
        .content_length()
        .is_some_and(|size| size > MAX_FILE as u64)
    {
        return Err(invalid("Skill download exceeds 1 MiB limit"));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| invalid(format!("Skill download body failed: {identity}")))?
    {
        if bytes.len() + chunk.len() > MAX_FILE {
            return Err(invalid("Skill download exceeds 1 MiB limit"));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

async fn fetch(
    client: &reqwest::Client,
    url: &mut reqwest::Url,
    identity: &str,
) -> Result<Vec<u8>, Error> {
    for attempt in 0..=3 {
        let response = client
            .get(url.clone())
            .send()
            .await
            .map_err(|_| invalid(format!("Skill download failed: {identity}")))?;
        if !response.status().is_redirection() {
            return read_response(response, identity).await;
        }
        if attempt == 3 {
            return Err(invalid("Skill URL exceeds redirect limit (3)"));
        }
        *url = redirect_url(url, &response)?;
    }
    Err(invalid("Skill download failed"))
}

fn literal_looking(source: &str) -> bool {
    source.contains('\n') || source.starts_with('#') || source.starts_with("---")
}

fn url_looking(source: &str) -> bool {
    source.split_once("://").is_some_and(|(scheme, _)| {
        !scheme.is_empty()
            && scheme
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.'))
    })
}
