//! Local workflow instructions. Content is data, never executable authorization.
mod library;
mod metadata;
mod source;
mod storage;

use crate::{config::manager::ConfigManager, error::Error, fs::OsFileSystem};
use clap::Subcommand;

/// Local skills command surface; the storage engine has no dependency on Clap.
#[derive(Debug, Clone, Subcommand)]
pub enum SkillsCommand {
    /// Summarize bundled and installed skills
    List {
        #[arg(long)]
        json: bool,
    },
    /// Read a skill, or the complete library
    Get {
        #[arg(required_unless_present = "all", conflicts_with = "all")]
        name: Option<String>,
        #[arg(long)]
        all: bool,
        #[arg(long)]
        full: bool,
        #[arg(long)]
        json: bool,
    },
    /// Import stdin (-), HTTP(S), an existing path, or literal Markdown
    Install {
        #[arg(allow_hyphen_values = true)]
        source: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        replace: bool,
    },
    /// Remove an installer-owned user skill
    Uninstall { name: String },
}

/// Dispatch local operations without executing skills or accessing API credentials.
///
/// # Errors
/// Returns structured validation errors or filesystem errors.
pub async fn execute(
    manager: &ConfigManager<OsFileSystem>,
    command: Option<&SkillsCommand>,
) -> Result<(), Error> {
    let config = manager.load_global_config()?;
    let root = storage::resolve_root(manager.config_dir(), &config.skills.directory)?;
    match command {
        Some(SkillsCommand::Install {
            source,
            name,
            replace,
        }) => {
            let installed = import_skill(&root, source, name.as_deref(), *replace).await?;
            crate::stdoutln!("Installed {installed}");
        }
        Some(SkillsCommand::Uninstall { name }) => {
            storage::uninstall(&root, name)?;
            crate::stdoutln!("Uninstalled {name}");
        }
        _ => print_discovery(manager, &root, command)?,
    }
    Ok(())
}

fn print_discovery(
    manager: &ConfigManager<OsFileSystem>,
    root: &std::path::Path,
    command: Option<&SkillsCommand>,
) -> Result<(), Error> {
    let (name, full, json, get) = match command {
        Some(SkillsCommand::Get {
            name, full, json, ..
        }) => (name.as_deref(), *full, *json, true),
        Some(SkillsCommand::List { json }) => (None, false, *json, false),
        _ => (None, false, false, false),
    };
    let entries = library::discover(root, name, full, get, &manager.list_specs()?)?;
    render(&entries, json, name.is_some())?;

    Ok(())
}

fn invalid(message: impl Into<String>) -> Error {
    Error::Internal {
        kind: crate::error::ErrorKind::Validation,
        message: message.into().into(),
        context: None,
    }
}

fn render(entries: &[library::Entry], json: bool, single: bool) -> Result<(), Error> {
    match (json, single) {
        (true, true) => {
            crate::stdoutln!("{}", serde_json::to_string_pretty(&entries[0])?);
        }
        (true, false) => {
            crate::stdoutln!("{}", serde_json::to_string_pretty(entries)?);
        }
        (false, _) => render_text(entries),
    }

    Ok(())
}
fn render_text(entries: &[library::Entry]) {
    for entry in entries {
        crate::stdoutln!("{} [{}]: {}", entry.name, entry.origin, entry.description);
        crate::stdoutln!(
            "  configured APIs: {}; missing APIs: {} (configuration only)",
            entry.configured_apis.join(", "),
            entry.missing_apis.join(", ")
        );
        render_identity(entry);
        if let Some(content) = &entry.content {
            crate::stdoutln!("{content}");
        }
        for (path, content) in &entry.files {
            crate::stdoutln!("\n--- {path} ---\n{content}");
        }
    }
}

async fn import_skill(
    root: &std::path::Path,
    input: &str,
    name: Option<&str>,
    replace: bool,
) -> Result<String, Error> {
    let imported = source::load(input).await?;
    storage::install(root, &imported, name, replace)
}

fn render_identity(entry: &library::Entry) {
    if let Some(revision) = entry.package_revision {
        crate::stdoutln!("  package revision: {revision}");
    }
    if let Some(provenance) = &entry.provenance {
        crate::stdoutln!(
            "  source: {} {:?}; content sha256: {}",
            provenance.source_type,
            provenance.source_identity,
            provenance.content_sha256
        );
    }
}
