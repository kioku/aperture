# Dependency feature size experiments

Chrono and Governor now enable only the features used by cache metadata and the
batch limiter. This removes 17 unused packages without upgrading any locked
version. Regex keeps its default acceleration engines: the larger size savings
in an experimental configuration caused a substantial search slowdown.

## Retained changes

- **Chrono:** `default-features = false`, `features = ["now"]`. Aperture only uses
  `Utc::now().to_rfc3339()` and stores the resulting string. `now` enables `std`;
  local timezone lookup, Chrono serde implementations and browser time bindings
  are unnecessary for the supported native CLI builds. Nine packages disappear
  from the lockfile, with no measured Linux binary saving.
- **Governor:** `default-features = false`, `features = ["std", "quanta"]`.
  Aperture uses a direct limiter and `until_ready()`, not keyed limiters or
  nonzero jitter. Retaining `quanta` preserves the existing `DefaultClock`;
  disabling it would switch to `MonotonicClock` and require a separate timing
  assessment. Eight packages disappear from the lockfile.

## Measured size

Linux x86_64, Rust 1.91.1, default/all features, stripped release profile:
`opt-level=z`, fat LTO, one codegen unit and panic-abort. Baseline source:
`85db320c62554a5aed361fcbe66f99d2c96b083f` (merged #248). Actual sizes and SHA-256
hashes, package removals and timing medians are in
[dependency-features.json](dependency-features.json).

| Configuration | Stripped bytes | Saving |
|---|---:|---:|
| Baseline default | 6,287,576 | — |
| Chrono only | 6,287,576 | 0 |
| Governor only | 6,275,280 | 12,296 |
| Retained changes, default | 6,275,280 | 12,296 (0.20%) |
| Baseline all features | 8,032,560 | — |
| Retained changes, all features | 8,020,264 | 12,296 (0.15%) |

This is a modest size improvement. Removing unused features does not guarantee
savings because the linker often already removes their code; Chrono demonstrates
that distinction. Dependency/build-graph reduction is a separate benefit.

## Rejected regex experiments

1. `default-features = false`, `features = ["std", "unicode"]` saved 340,008 bytes
   (332 KiB). CLI output was unchanged, but searching `regex:\p{Greek}+` over
   1,000 operations with long descriptions increased median latency from 49.81
   to 303.15 ms (6.1×). Correct results alone do not make this acceptable.
2. Retaining `std`, `unicode`, `perf-dfa`, `perf-inline`, `perf-literal` and
   `perf-cache`, while excluding one-pass/backtracking acceleration, saved only
   12,288 bytes. The Greek case returned to 45.95 vs 46.21 ms in its matched run.
   That small gain does not justify adding an engine-performance trade-off, so
   defaults remain unchanged.

Unicode classes, case folding and supported regex syntax were preserved in both
experiments. Tracing's environment filter independently enables regex-automata
DFA support. Cargo feature union also matters for all-feature builds: `oas3`
requests regex defaults, so narrowing Aperture's direct regex declaration does
not narrow every configuration. No claim is made that all attributed regex code
can be removed without a supported-behavior change.

## Method and validation

Each isolated experiment started from the original manifest/lockfile, with no
version upgrades or compiler/linker changes. The combined retained configuration
was built separately with default and all features. Startup cases use synthetic
10/1,000/5,000-operation specs, first/last selected operation, five warmups and 35
shuffled/interleaved fresh-process samples per variant/case. Search cases use
1,000 operations with 4,122-byte descriptions, three warmups and 15 interleaved
samples per variant/query. Compare timings only with the baseline in that same
run; these are local warm-filesystem observations, not cross-platform guarantees.
No startup-speed improvement is claimed.

All probes used temporary isolated `APERTURE_CONFIG_DIR` values and help/search
or dry-runs against loopback port 9; no real credentials or remote APIs were used.
CLI comparisons checked exit code, stdout and stderr, including Unicode queries,
invalid patterns, unsupported lookahead, empty/sentinel values and unresolved
commands. A new search regression asserts actual Unicode matches rather than
only checking determinism. Existing cache-fingerprint and batch rate-limit tests
cover the retained features. Repository tests, formatting, Clippy and enabled
commit hooks remain required for the delivery head.

Size/startup reproduction uses the same release settings and existing script:

```sh
cargo build --release --locked
cp target/release/aperture /tmp/aperture-features-default
cargo build --release --locked --all-features
python3 scripts/benchmark-startup.py /tmp/aperture-features-default \
  --output /tmp/aperture-features-startup.json
```

The summary JSON records the additional search fixture shape and exact query
strings. Raw samples, per-candidate manifests/lockfiles, isolated comparison
script, retained binaries and logs from this experiment are under
`/tmp/aperture-dep-features/` on the measurement host; that temporary evidence is
not required to interpret the committed medians and size conclusions.
