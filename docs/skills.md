# Workflow skills

Aperture hosts reusable workflow instructions alongside API discovery. Skills are Markdown and optional reference/template files, **not executable plugins or permission grants**. Importing or reading a skill never calls an API, runs a shell/hook, changes credentials, or downloads dependencies.

## Discover and read

```sh
aperture skills                         # same as skills list
aperture skills list --json
aperture skills get core --full
aperture skills get release --json
aperture skills get release --full --json
aperture skills get --all --full --json
```

List summarizes the library; get returns Markdown. `--full` adds reference/template files in sorted path order; `--all` reads the entire library and cannot be combined with a name. JSON get with a name returns one object; list and get-all return arrays sorted by identifier. Text output includes origin and required-API configuration status. JSON includes provenance and the embedded core's package revision. References are represented by `{ "encoding": "utf-8", "content": "..." }`, or explicitly base64-encoded if their bytes are not UTF-8. Reference filenames must be UTF-8; non-UTF-8 filenames fail safely rather than being renamed lossily. The same encoding label appears in text full output.

The embedded `core` skill ships inside Cargo, Nix, and standalone binaries. It needs no companion files or network and matches the running package. Use `aperture build-info --json` for full package/source identity. Installed skills have **no CLI-version compatibility guarantee**: inspect current help and `--describe-json` before using their examples. `core` is protected from install, replacement, and uninstall, including case-insensitive collisions.

`aperture.required_apis` reports `required_apis`, `configured_apis`, and `missing_apis`, comparing names with locally installed API specs. `readiness: "local_configuration_only"` means exactly that: it does not verify credentials, authentication, remote availability, or service health. No credentials are read for discovery or URL installation. Ordinary skills need no API metadata.

## Configure a generic user library

The default is `<configuration directory>/skills`. The configuration directory follows normal Aperture resolution, including `APERTURE_CONFIG_DIR`.

```sh
aperture config set skills.directory workflows
aperture config get skills.directory
aperture config setting list
```

Native TOML:

```toml
[skills]
directory = "workflows"
```

Relative paths resolve against the **configuration directory**, not the current working directory. Absolute paths are supported. Paths are literal: no environment-variable or tilde expansion; expand them explicitly in your shell if desired. Empty paths, `~` prefixes, and parent traversal are rejected. The explicitly configured library root (including a root alias) is trusted and canonicalized, or its nearest existing ancestor is pinned when the library is absent. Read-only core discovery does not create the library. Dangling root aliases fail; symlinks below the pinned boundary remain forbidden. The library is independent of API specs, caches, secrets, and agent tool directories. This setting does not install anything into an agent's configuration.

## Import and remove

```sh
aperture skills install ./release-workflow/
aperture skills install ./SKILL.md
aperture skills install https://example.com/SKILL.md
aperture skills install '# Release workflow' --name release
cat SKILL.md | aperture skills install - --name release
aperture skills install ./updated-workflow/ --name release --replace
aperture skills uninstall release
aperture skills install ./SKILL.md --name release --json
aperture skills uninstall release --json
```

Mutation `--json` success output is an object with exactly `status` (`"installed"` or `"uninstalled"`) and `name` (the installed identifier), both strings. It contains no skill content. Ordinary output remains `Installed <name>` / `Uninstalled <name>`; global `--json-errors` controls failures separately.

The positional source is detected in this order:

1. Exactly `-`: bounded stdin.
2. A URL-like scheme followed by `://`: HTTP(S) only. Invalid URLs and other schemes fail. Markdown containing a URL is still Markdown unless it starts with a URL-like scheme.
3. An existing local path: regular file or directory. Existing paths win over ambiguous plain prose; use stdin to force literal content.
4. Literal Markdown. Missing path-looking inputs fail instead of installing the path spelling. Path-looking means a slash/backslash, a leading dot/tilde, a `.md` extension (case-insensitive), or a colon. Multiline content, headings starting `#`, and frontmatter starting `---` are literal-looking instead. Use `./name` for a missing extensionless path; a bare word otherwise counts as literal prose and needs `--name`.

Existing source ancestors may be aliases: the selected regular file/directory is canonicalized before reading, but a selected source symlink or dangling alias is rejected. A directory must contain root `SKILL.md`; other regular files retain their relative hierarchy. Empty directories are not preserved. Selected source entries that are symlinks, nested symlinks, special files, traversal, unsafe Windows filenames, and case-insensitive reference collisions are rejected. Filenames may contain spaces or Unicode, but not control characters, Windows punctuation, trailing dots/spaces, or reserved device stems such as `CON.txt`.

### Metadata, names, and fallbacks

```yaml
---
name: release
description: Plan and verify an approved release.
aperture:
  required_apis: [source, tracker]
---
# Release

Discover the current operation names and flags before constructing calls.
```

Standard unknown frontmatter fields (including unknown `aperture` fields) are accepted and preserved in Markdown. Known fields are strict: name/description must be nonempty strings; aperture must be a mapping; required_apis must be an array of valid, unique Aperture API context names. Nulls, wrong types, duplicate YAML keys, malformed YAML, unterminated frontmatter, empty documents, and empty frontmatter bodies fail without silently falling back. Plain Markdown without frontmatter remains supported.

The installed identifier is chosen from `--name`, then frontmatter `name`, then a local file's stem or directory's basename. Stdin, URL, and literal imports without frontmatter require `--name`. Fallback names are used exactly, without guessing or normalization: `Release.md` needs `--name release`; plain `SKILL.md` needs a frontmatter name or `--name`. Identifiers contain 1–64 lowercase ASCII letters, digits, hyphens, or underscores, with no separators/traversal or Windows device names. A name override changes the installed identifier, not the original Markdown/frontmatter. Invalid known metadata still fails even when `--name` is supplied.

Description defaults to the first `# ` heading, otherwise `User workflow instructions`. Full and list output are deterministic for unchanged library/configuration state.

### Limits and URL safety

Each file/document is limited to **1 MiB**, directory content to **8 MiB**, directory entries (files plus directories) to **128**, and nested directory depth to **16**. Installer metadata is separate from these content budgets and is itself bounded to 1 MiB. UTF-8 is required for SKILL.md, not for reference bytes. No file is executed or recursively interpreted as a dependency.

URLs download **one Markdown document only**. Downloads have a 15-second per-request timeout, a 20-second overall deadline, and at most three redirects. Redirect destinations must also be HTTP(S) and credential-free. HTTP status failures, invalid UTF-8, oversized bodies, and network errors fail without installing. The downloader uses a separate client with no API authentication, cookies, configured API proxy credentials, or environment proxies. URL username/password credentials are forbidden. Query/fragment are omitted from stored provenance and diagnostics; the requested URL may still use a query to identify the document. Do not treat this as permission to publish secrets in URLs.

No registries, source flags, archives, updates, reference/dependency downloads, package installations, or executable hooks are supported.

### Ownership, provenance, and transactions

Each installed directory has a separate `.aperture-skill.json` with format version, installed name, source type, safe source identity, and SHA-256 of the imported content snapshot. The hash includes sorted relative filenames and length-prefixed bytes, not installation time or bundled build metadata. It records imported content; it is not a signature or a claim that later manual edits are authentic. Local provenance and content can be modified by the library owner; ownership metadata is not an authorization mechanism.

Installation validates the full snapshot before writing. Cooperating processes serialize reads/mutations using a library filesystem lock. Files are written to an exclusively created staging directory on the destination filesystem and synced, then published with native rename. Replacement requires `--replace` and valid installer ownership, retains the old directory in a temporary backup, and restores it if the publish rename fails. A restore failure reports the retained backup path. Readers using the CLI do not observe the replacement rename gap. Uninstall validates ownership and all paths, moves only the selected directory aside under the lock, then removes it. Neither operation derives deletion paths from provenance.

Manually placed `<library>/<name>/SKILL.md` skills can be listed/read, but cannot be replaced or uninstalled by the CLI without valid installer metadata. Handle them manually, or import their content under another name. Do not copy installer metadata into an import source; it is reserved. Corrupt selected skills fail explicitly; selecting `core` does not inspect unrelated installed content. Listing/get-all fails rather than silently hiding corrupt entries.

These are local, cooperating-process transactions, not a database or a power-loss recovery protocol. A crash during replacement may leave a hidden `.skill-stage-*` backup; inspect it before manually restoring anything. Hostile local processes racing filesystem changes are outside the advisory-lock boundary. Keep the library and source directories under trusted local ownership; imported instructions themselves remain untrusted.

## Agent integration and example

[skills/aperture/SKILL.md](../skills/aperture/SKILL.md) is a thin stable discovery stub with Aperture, API, workflow, and troubleshooting triggers. **Manually copy** that folder to your agent's supported skill directory. There is no npm/registry dependency and no automatic agent-directory mutation in v1. The stub tells the agent to discover the running CLI's `core` and local workflows at runtime, avoiding stale copies of version-specific instructions.

[skills/examples/release.md](../skills/examples/release.md) is a practical workflow example with required API metadata, current operation discovery, dry runs, approval boundaries, and result verification. Adapt its required API names to your local configuration and import it with `aperture skills install ./skills/examples/release.md`. It intentionally does not invent installation-specific mutation commands.

## Troubleshooting

- Missing name: add frontmatter or `--name`; names are never generated from downloaded content.
- Missing source: verify the path. Use stdin for text that resembles a path.
- Already exists: review the existing skill, then use explicit `--replace` if it is installer-owned.
- Manual/corrupt ownership: do not fabricate metadata to force deletion; inspect the directory manually.
- Missing required API: configure its spec using current `aperture config --help`; then separately verify authentication/health with an approved read operation.
- Unsafe path/symlink: copy regular files into a trusted non-symlink directory. Do not weaken the path checks.
- Download failure: verify HTTP status, UTF-8, redirect count, size, and deadline. Install a reviewed local document instead if needed; no API credentials are sent to fetch it.
