# Large-spec startup (#233)

Targeted CLI invocations build only the selected operation's Clap tree. Selection
uses the same global flag grammar and mapped names/aliases as batch execution.
Root/group discovery and unresolved paths retain the full tree for help and
suggestions. Required-parameter validation, positional mode and examples fallback
still use the generated operation's existing parser. This removes whole-tree
allocation, not whole-spec loading: decoding, freshness hashing and operation
lookup still scale with specification size.

## Cache correctness

The loader reads global cache metadata once per load. That snapshot supplies the
source fingerprint; the binary's own embedded format version is authoritative,
even if metadata is missing, corrupt or has a different advisory version.
Size/mtime differences reject the cache without hashing. Matching size/mtime
**still require SHA-256 of the source**: equal-size edits with restored timestamps
must invalidate caches. Missing fingerprints and unavailable source files retain
the existing legacy pass-through behavior. This change does not trust timestamps
alone or change cache layout/version.

## Reproduction

Baseline: `f06b7a37323a96a11afaf21caabe1c9f33f26e19`.
Linux x86_64, Rust 1.91.1, Cargo release profile: `opt-level=z`, fat LTO,
`codegen-units=1`, `panic=abort`, stripped. Default and all-feature binaries use
identical settings before/after; preserve each binary before building the next.

```sh
cargo build --release --locked
cp target/release/aperture /tmp/aperture-default
cargo build --release --locked --all-features
cp target/release/aperture /tmp/aperture-all
python3 scripts/benchmark-startup.py /tmp/aperture-default --output /tmp/default.json
python3 scripts/benchmark-startup.py /tmp/aperture-all --output /tmp/all.json
```

The script initializes temporary isolated `APERTURE_CONFIG_DIR` data, generates
10/1,000/5,000-operation synthetic JSON specs, warms each case five times, and
measures 35 fresh-process dry-runs. First/last-operation selections expose lookup
position costs. Timing includes process startup and warm filesystem reads.
Seven separate GNU time runs measure direct-child peak RSS; measuring through
Python's child `ru_maxrss` can introduce a parent-memory floor, so it is not used.
No real credentials, network requests or default-config mutations are needed.

## Results

Medians; RSS in MiB (GNU time KiB / 1,024). Raw timing/memory samples, compiler
identity, binary SHA-256 and exact byte counts are in [gh-233.json](gh-233.json).

| Features | Operations | Selected | Before ms | After ms | Before RSS | After RSS |
|---|---:|---|---:|---:|---:|---:|
| Default | 10 | First | 4.18 | 3.84 | 5.24 | 5.21 |
| Default | 10 | Last | 4.01 | 3.99 | 5.25 | 5.27 |
| Default | 1,000 | First | 15.95 | 6.21 | 13.87 | 6.22 |
| Default | 1,000 | Last | 15.97 | 7.67 | 14.07 | 6.07 |
| Default | 5,000 | First | 68.57 | 17.03 | 50.17 | 11.25 |
| Default | 5,000 | Last | 69.08 | 22.15 | 50.30 | 11.20 |
| All | 10 | First | 3.99 | 3.85 | 5.43 | 5.55 |
| All | 10 | Last | 4.22 | 3.91 | 5.31 | 5.41 |
| All | 1,000 | First | 16.51 | 6.33 | 14.04 | 6.40 |
| All | 1,000 | Last | 15.96 | 7.34 | 14.08 | 6.41 |
| All | 5,000 | First | 65.40 | 16.35 | 50.23 | 11.37 |
| All | 5,000 | Last | 67.94 | 22.13 | 50.32 | 11.12 |

Default 5,000-operation latency improves 68–75%; peak memory improves about 78%.
These are local warm-process observations, not cold-start, server-latency or
cross-platform guarantees. Small-case differences are too small for strong claims.

| Stripped binary | Before bytes | After bytes |
|---|---:|---:|
| Default | 6,287,576 | 6,287,576 |
| All features | 8,032,560 | 8,032,560 |

## Dependency / regex assessment

Removed unused optional direct `ahash` and its jq feature entry. Source has no
ahash consumer, and the current jaq dependencies do not require it. Its previous
unused code was already eliminated by linking: **no measured binary saving**.

Spec normalization is already structural (`src/spec/normalization.rs`), so regex
is not needed for parser compatibility. The direct regex dependency remains for
explicit regex search (`src/search.rs`); ordinary keyword/fuzzy search does not
compile patterns. Removing regex or disabling syntax/Unicode support would change
that supported interface and is not justified by this startup optimization.

Separate default-release `cargo bloat --release --locked --crates
--message-format json` attribution, with symbols retained by cargo-bloat, is
unchanged before/after: regex_automata 313,671 bytes, aho_corasick 116,484 bytes,
regex_syntax 106,473 bytes (536,628 total; about 524 KiB). This is linked text
attribution under the same optimization profile, not a guaranteed removable size
or a substitute for the actual stripped-binary measurements above.
