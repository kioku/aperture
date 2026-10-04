# User Guide

This guide covers day-to-day usage of Aperture for interacting with APIs.

## Basic Workflow

### 1. Register an API

```bash
# From a local file
aperture config add my-api ./openapi.yaml

# From a URL
aperture config add my-api https://api.example.com/openapi.yaml
```

### 2. Explore Available Commands

```bash
# List registered APIs
aperture config list

# Land in an API context (overview + next actions)
aperture api my-api

# List commands for an API
aperture commands my-api

# Get detailed command information (machine-oriented)
aperture api my-api --describe-json
```

### Structured Discovery Output

For script-friendly discovery data, use structured output modes:

```bash
aperture commands my-api --format json
aperture overview my-api --format json
aperture docs my-api --format json
aperture docs my-api users get-user-by-id --format json
aperture config api list --json
```

Top-level response shapes are stable for scripting:

- `commands --format json` → `{ api, groups[] }`
- `overview <api> --format json` → `{ api, statistics, quick_start, sample_operations[] }`
- `overview --all --format json` → `{ apis[] }`
- `docs --format json` → `{ mode: "interactive", apis[] }`
- `docs <api> --format json` → `{ mode: "api-reference", api, categories[], example_paths[] }`
- `docs <api> <tag> <operation> --format json` → `{ mode: "operation", api, operation }`
- `config api list --json` → `[{ name, ... }]` (`--verbose` adds endpoint details)

### Shell Completion

Aperture can generate completion scripts for major shells:

```bash
aperture completion bash
aperture completion zsh
aperture completion fish
aperture completion nu
aperture completion powershell
```

Install generated scripts:

```bash
# bash
aperture completion bash > ~/.local/share/bash-completion/completions/aperture

# zsh
mkdir -p ~/.zfunc
aperture completion zsh > ~/.zfunc/_aperture

# fish
aperture completion fish > ~/.config/fish/completions/aperture.fish

# Nushell
aperture completion nu > ~/.config/nushell/completions/aperture.nu
# then add this to ~/.config/nushell/config.nu if not already sourced:
source ~/.config/nushell/completions/aperture.nu

# PowerShell
aperture completion powershell | Out-String | Invoke-Expression
```

Completion behavior:

- completes static top-level and `config` command paths,
- completes configured API context names,
- completes dynamic groups, operations, and operation flags under `aperture api <context> ...`.

Known trade-offs:

- dynamic suggestions are driven by local cached specs; refresh cache after spec updates,
- completion suggests command/flag names, not parameter values.

### 3. Execute Commands

```bash
# Flag-based syntax (default)
aperture api my-api users list
aperture api my-api users get-user-by-id --id 123
aperture api my-api users create --name "John Doe" --email "john@example.com"
```

## CLI Naming Conventions

Aperture follows these naming rules across commands, subcommands, and examples:

- **Top-level commands:** prefer clear, full words (`commands`, `run`, `search`, `docs`, `overview`).
- **Config subcommands:** use `verb-resource` (`set-url`, `get-url`, `list-secrets`).
- **Examples and docs:** always use canonical command names.

### Compatibility aliases

To avoid breaking existing scripts, Aperture supports these legacy aliases:

- `aperture list-commands` → `aperture commands`
- `aperture exec` → `aperture run`

These aliases remain available for compatibility, but new usage should prefer canonical names.

## Command Syntax

Aperture uses flag-based syntax for all parameters:

```bash
aperture api <api-name> <tag> <operation> [--param value ...]
```

**Examples:**

```bash
# Path parameters
aperture api my-api users get-user-by-id --id 123

# Query parameters
aperture api my-api users list --limit 10 --offset 0

# Request body (inline JSON)
aperture api my-api users create --body '{"name": "John", "email": "john@example.com"}'

# Request body from a file (avoids shell-quoting issues with large payloads)
aperture api my-api users create --body-file ./payload.json

# Request body from stdin
echo '{"name": "John"}' | aperture api my-api users create --body-file -

# Multiple parameters
aperture api my-api orders search --status pending --created-after 2024-01-01
```

### Binary request and response bodies

Aperture supports explicitly modeled single-part binary media (including `application/octet-stream`, image, and PDF) declared by OpenAPI as `type: string`, `format: binary`. Multipart, form, XML, JSON, and text media are not inferred as raw bytes. Upload bytes with `--body-file PATH`, or use
`--body-file -` to read stdin. Inline `--body` remains JSON-only.

Declared binary responses require an explicit destination:

```bash
aperture api --output-file download.bin my-api blobs download-blob
aperture api --output-file - my-api blobs download-blob > download.bin
```

Both destinations preserve bytes exactly and add no newline. `--dry-run` never creates the
output file. Binary operations cannot use `--cache`; binary responses also cannot use `--jq`,
normal response formatting, auto-pagination, or batch response capture. Batch `body_file`
continues to support uploads and reads raw bytes for binary operations.

### Flag Scoping Model

Execution-oriented flags are scoped to execution commands (`api`, `run`) instead of being global.

```bash
# Supported on execution commands
aperture api my-api --dry-run users get-user-by-id --id 123
aperture run --dry-run getUserById --id 123

# Not allowed on non-execution commands
aperture docs --dry-run
```

Universal flags remain available everywhere: `--json-errors`, `--quiet`, and `-v`.

### Legacy Positional Syntax

For backwards compatibility, positional arguments are available:

```bash
# Enable with --positional-args
aperture api my-api --positional-args users get-user-by-id 123
```

## Output Formats

### JSON (Default)

```bash
aperture api my-api users get-user-by-id --id 123
```

### YAML

```bash
aperture api my-api users get-user-by-id --id 123 --format yaml
```

### Table

```bash
aperture api my-api users list --format table
```

## Response Filtering

Use the `--jq` flag to extract specific fields from responses.

### Basic Filtering (Always Available)

```bash
# Single field
aperture api my-api users get-user-by-id --id 123 --jq '.name'

# Nested field
aperture api my-api users get-user-by-id --id 123 --jq '.address.city'

# Array index
aperture api my-api users list --jq '.users[0]'
```

### Advanced Filtering (Requires `--features jq`)

Build with JQ support for advanced queries:

```bash
cargo install aperture-cli --features jq
```

Then use full JQ syntax:

```bash
# Filter array elements
aperture api my-api users list --jq '[.users[] | select(.active == true)]'

# Transform output
aperture api my-api users list --jq '.users | map({id, name})'

# Count results
aperture api my-api users list --jq '.users | length'
```

## Response Caching

Cache responses to reduce redundant API calls:

```bash
# Enable caching with default TTL (300 seconds)
aperture api my-api --cache users list

# Custom TTL (in seconds)
aperture api my-api --cache --cache-ttl 600 users list

# Disable caching explicitly
aperture api my-api --no-cache users list
```

Response caching applies only to GET and HEAD. Unsafe methods such as POST always execute; there is no unsafe-method caching opt-in. Cached results retain status and response headers for pagination, but responses that set session cookies are not cached. Dry runs always return a request plan and never read or create the response cache.

Authenticated origin requests and selected proxy configurations containing credentials (including scheme-less proxy authorities such as `user:password@host:port`) bypass response-cache reads and writes, even with the legacy authenticated-cache opt-in. This conservative proxy policy also applies to destinations excluded by `NO_PROXY`; explicit `--no-proxy` disables that proxy policy. Anonymous proxy requests remain cacheable. Executor cache keys include a proxy-policy revision so older, potentially account-mixed entries are missed without deleting them. Environment proxy settings take precedence over configured proxies, and explicit CLI proxy overrides take precedence over both.

### Cache Management

```bash
# View cache statistics
aperture config cache-stats my-api

# Clear cache for an API
aperture config clear-cache my-api
```

## Command Mapping

Customize the CLI command tree without modifying the OpenAPI spec. Rename groups, rename operations, add aliases, or hide commands.

### Rename a Tag Group

```bash
# Rename "User Management" to "users"
aperture config set-mapping my-api --group "User Management" users
aperture config reinit my-api
```

### Rename an Operation

```bash
# Rename getUserById to "fetch"
aperture config set-mapping my-api --operation getUserById --name fetch
aperture config reinit my-api

# Now use the new name
aperture api my-api users fetch --id 123
```

### Add Aliases

```bash
# Add "get" and "show" as aliases for an operation
aperture config set-mapping my-api --operation getUserById --alias get
aperture config set-mapping my-api --operation getUserById --alias show
aperture config reinit my-api

# All three work
aperture api my-api users get-user-by-id --id 123
aperture api my-api users get --id 123
aperture api my-api users show --id 123
```

### Move an Operation to a Different Group

```bash
# Move getUserById from its original tag group to "accounts"
aperture config set-mapping my-api --operation getUserById --op-group accounts
aperture config reinit my-api
```

### Hide an Operation

```bash
# Hide a deprecated or internal operation from help output
aperture config set-mapping my-api --operation deleteUser --hidden
aperture config reinit my-api

# The command still works but doesn't appear in --help
aperture api my-api users delete-user --id 123
```

### View and Remove Mappings

```bash
# List all mappings
aperture config list-mappings my-api

# Remove an operation mapping
aperture config remove-mapping my-api --operation getUserById

# Remove a group mapping
aperture config remove-mapping my-api --group "User Management"

# Apply changes
aperture config reinit my-api
```

> **Note:** All mapping changes require `aperture config reinit` to take effect. The mappings are applied during cache generation.

## Search Commands

**Canonical role:** primary intent-first discovery. Use search when you know what you want to do but not where the command is.

Search matches operation names, descriptions, display names, and aliases from command mappings.
Ordinary queries use case-insensitive keyword/fuzzy matching. Operation-name
matching ignores punctuation and tolerates a single edit or adjacent transposition
for queries of four or more characters. Exact names rank ahead of typo matches.
Use `regex:<pattern>` for explicit regex search (case-sensitive unless the pattern
uses `(?i)`); invalid patterns produce an error. Ties sort by API and command path.

```bash
# Search by keyword
aperture search "create user"

# Search within specific API
aperture search "list" --api my-api

# Regex search
aperture search "get.*by.*id" --verbose

# Finds operations by display name or alias
aperture search "fetch"
```

## Shortcuts

Execute operations using shorthand:

```bash
# By operation ID
aperture run getUserById --id 123

# By HTTP method and path
aperture run GET /users/123

# By tag and operation
aperture run users list
```

## API Exploration

Use this human workflow for discovery:

1. **Land** with `api <context>`
2. **Find** with `search`
3. **Inspect** with `docs`
4. **Execute** with `api <context> <tag> <operation>`

### Overview

**Canonical role:** orientation. Get a high-level API summary with statistics and starter paths.

```bash
# Overview of a specific API
aperture overview my-api

# Overview of all registered APIs
aperture overview --all
```

### Commands Tree

**Canonical role:** structural lookup. Get a terse tree of available command paths.

```bash
# Canonical command name
aperture commands my-api

# Legacy alias
aperture list-commands my-api
```

### Interactive Documentation

**Canonical role:** deep reference. Inspect exact operation usage, parameters, request bodies, and responses.

```bash
# Interactive help menu
aperture docs

# API reference index
aperture docs my-api

# Detailed command help with parameters and examples
aperture docs my-api users get-user

# Enhanced formatting with tips
aperture docs my-api users get-user --enhanced
```

## Exit Codes

| Code | Meaning |
|------|---------|
| `0` | Success |
| `1` | Failure (API error, network error, validation error) |

Use exit codes in scripts:

```bash
if aperture api my-api users get-user-by-id --id 123; then
    echo "User found"
else
    echo "Request failed"
fi
```

## Environment Variables

| Variable | Description |
|----------|-------------|
| `APERTURE_BASE_URL` | Global base URL override |
| `APERTURE_ENV` | Environment selector (e.g., `staging`, `prod`) |
| `RUST_LOG` | Log level (`debug`, `info`, `warn`, `error`) |

## Common Patterns

### Piping to Other Tools

```bash
# Pretty print with jq
aperture api my-api users list | jq .

# Save to file
aperture api my-api users list > users.json

# Process with other tools
aperture api my-api users list --jq '.users[].email' | sort | uniq
```

### Scripting

```bash
#!/bin/bash
set -e

# Fetch and process users
USERS=$(aperture api my-api --json-errors users list --jq '.users')
COUNT=$(echo "$USERS" | jq 'length')

echo "Found $COUNT users"

# Iterate over results
echo "$USERS" | jq -c '.[]' | while read -r user; do
    ID=$(echo "$user" | jq -r '.id')
    NAME=$(echo "$user" | jq -r '.name')
    echo "Processing user $ID: $NAME"
done
```

### CI/CD Integration

```yaml
# GitHub Actions example
- name: Fetch deployment status
  run: |
    aperture api deploy-api --json-errors status get --env production
  env:
    DEPLOY_API_TOKEN: ${{ secrets.DEPLOY_API_TOKEN }}
```

## Request behavior

### Pagination completeness and next links

Automatic pagination follows both the path and query of `Link: ...; rel="next"`
URLs, including relative links resolved against the current page. Next links
must retain the original scheme, host, and effective port, and must not contain
URL credentials or fragments. Operation authentication and headers are retained
only for those same-origin requests; cross-origin links return an error before
sending a request. Pagination rejects HTTP redirects (including same-origin
redirects); the server must provide a validated next link instead. This keeps
relative-link resolution and custom authentication headers within this policy.
Link lists preserve commas in URLs and quoted attributes, support relation-token
lists, and reject malformed or ambiguous next targets rather than silently
reporting completion. Multiple Link header fields are treated as one list.
One traversal reuses its HTTP connection pool.

Pagination uses shared, context-scoped HTTP clients keyed by transport settings
and redirect policy. Strict pagination rejects all redirects, including
same-origin redirects. Its response-cache identity is separate from ordinary
requests that follow redirects, so a warmed ordinary client or cached redirect
cannot bypass pagination boundaries. Direct cached pagination responses retain
Link headers and continue to avoid network requests on repeated traversals.

Repeated page URLs/cursors and the 1,000-page safety cap return an incomplete
traversal error when more data remains. Already emitted NDJSON is partial output.
A closed output pipe stops pagination without fetching another page.

### OpenAPI URL parameter serialization

Path parameters use their declared `simple`, `label`, or `matrix` style and
`explode` setting. The defaults are `simple` and `explode=false`. Each data
component is percent-encoded before style punctuation is inserted, so commas,
semicolons, equals signs, slashes, question marks, hashes, percent signs, spaces,
and Unicode remain data. Label expansion also encodes data dots when exploding.

The CLI accepts scalar strings directly. For array/object parameters, pass a
JSON array/object of non-null primitive values, for example
`--id '["blue","black"]'` or `--id '{"a":"blue","b":"black"}'`.
Arrays/objects retain their declared wire representation rather than sending
the JSON source text. Primitive number, integer, and boolean inputs must match
the declared type; item/property primitive types are checked too. Local
`#/components/schemas/` references are resolved, including array items and object
properties. Empty/description-only schemas retain opaque string input; string-only
compositions also use scalar encoding. These codecs select a wire shape, not full
JSON Schema validation. Examples/defaults are preserved as payload data.
Object keys are sorted for deterministic URLs. Duplicate JSON object keys keep
the last value, as in the JSON decoder; duplicate array values are preserved.
Unknown object properties are accepted unless their primitive type is declared.
Empty collections are omitted.

Query parameters use structured URL pairs with encoded keys and values.
`form` defaults to `explode=true`: arrays produce repeated keys and objects
produce separate property keys. Unexploded form uses comma-separated values;
`spaceDelimited` and `pipeDelimited` support arrays with `explode=false`.
`deepObject` supports flat objects with explicit `explode=true`.

Unsupported representations return validation errors before a request is sent:
content-based parameters; missing/cyclic/external schema references; ambiguous
compound compositions and composed item/property schemas; null or nested compound values;
`allowReserved=true`; exploded delimited query arrays; ambiguous delimiter data
inside unexploded/delimited query values; and bracket-containing deep-object
property names. Use exploded form when query data contains its delimiter.
Dot-only path segments (including label expansion that produces `.` or `..`)
are rejected because URL parsers would normalize them and change the path.

Cached specifications retain these declarations in parsed-spec cache format version 9.
This replaces layout 8 (grouped security) and is separate from response-cache keys.
Older binary caches must be regenerated; JSON fixtures without serialization
metadata retain the OpenAPI location defaults.

### Transport retry policy

Transport retries include truncated response-body reads even when the HTTP
client wraps them as decoding errors. Unrelated decoding, builder, redirect,
and terminal HTTP failures are not transport retries. Exhaustion reports the
last attempt rather than an earlier HTTP response. Non-idempotent SDK requests
require a non-empty idempotency key actually present in the request headers or
explicit force-retry; an eligibility flag alone does not authorize a retry.
Transport error messages omit request URLs to avoid exposing URL credentials;
SDK and CLI JSON errors retain their network classification.

## Troubleshooting

### Debug Mode

```bash
RUST_LOG=debug aperture api my-api users list
```

### Dry Run

Preview requests without executing:

```bash
aperture api my-api --dry-run users create --name "Test"
```

### Validate Spec

Re-add with strict mode to check for issues:

```bash
aperture config add --strict my-api ./openapi.yaml
```

### Clear and Reinitialize

```bash
# Clear cache for specific API
aperture config clear-cache my-api

# Reinitialize all cached specs
aperture config reinit --all

# Reinitialize specific spec
aperture config reinit my-api
```

A cache-format mismatch after an Aperture upgrade is intentional: older transformed
spec caches may not contain every response variant needed for safe execution. Run
`aperture config reinit my-api` (or `--all`) to regenerate them from the stored source
spec. Aperture rejects the old cache and does not modify it during ordinary API calls.

### Execution defaults

`default_timeout_secs` applies to SDK contexts carrying global configuration and
CLI requests, including batch operations. `--timeout-secs N` overrides it and
accepts 1 through 31,536,000 seconds, matching the configuration setting's range.
`agent_defaults.json_errors` applies to command-usage, argument-parsing, and
missing-API errors. `--json-errors` forces JSON; `--json-errors=false` forces text.
Help and version output retain their normal text format.
Query/header boolean arguments accept `--enabled true` or `--enabled false`.
A bare `--enabled` means true; an omitted optional parameter is not sent.

### Batch allocation benchmark

Batch execution borrows one immutable specification and polls at most
`--batch-concurrency` operation futures. It builds a clap tree for only the
selected operation. Zero concurrency and values exceeding Tokio's supported
semaphore limit are rejected by CLI parsing and SDK execution.
The input operations and returned results still require memory proportional to
batch length; queued operations no longer each own a full specification/tree.

A local Linux debug-build dry-run benchmark on 2026-10-03 used a 1,000-operation
specification and concurrency 1 (the ignored
`large_spec_fixed_concurrency_memory_benchmark` test, with
`CLI_BATCH_BENCH_OPS` selecting the batch length):

| Batch operations | Peak RSS (KiB) | Wall time (s) |
| ---: | ---: | ---: |
| 1 | 11,336 | 0.16 |
| 25 | 11,948 | 2.21 |
| 100 | 12,304 | 8.38 |
| 1,000 | 16,888 | 97.77 |

These measurements include retained results, use synthetic operations, and ran
while compilation was active. They demonstrate allocation scaling within this
setup; timings are not release-build or cross-platform performance guarantees.
