# ADR 010: Dependent Batch Workflows

## Status

Accepted

## Context

The batch processing system (§9.1 of the architecture doc) executes multiple API
operations concurrently from a single batch file. However, real-world agent workflows
are frequently sequential with data dependencies between steps — for example,
"Create User → capture the returned ID → Get User by ID → Add User to Group."

Without dependency support, agents must issue individual `aperture` invocations for
each step, parse intermediate results, and construct subsequent calls. This
introduces per-step latency from the agent ↔ tool roundtrip and pushes orchestration
complexity into the agent.

The design space included three broad approaches:

1. **External orchestration** — keep the batch system independent; let the agent or a
   shell script handle sequencing. Simplest for Aperture, but shifts all complexity to
   the caller and multiplies invocation overhead.
2. **In-batch dependency DSL** — extend the batch file format with dependency
   declarations, variable capture, and interpolation so that multi-step workflows
   execute in a single invocation.
3. **Workflow engine** — build a full DAG executor with conditional branches,
   loops, and retry-per-step. Powerful, but significantly more complex than the
   problem requires.

## Decision

We chose option 2: an in-batch dependency DSL. Three optional fields are added to
`BatchOperation`:

- **`capture`** (`HashMap<String, String>`): scalar value extraction via JQ queries.
- **`capture_append`** (`HashMap<String, String>`): list accumulation via JQ queries
  for fan-out/aggregate patterns.
- **`depends_on`** (`Vec<String>`): explicit dependency declaration on other
  operations by `id`.

The batch processor auto-detects whether any operation uses these fields and switches
from concurrent to sequential execution. Existing batch files are unaffected.

### Key design choices

**Automatic execution path selection.** Rather than requiring a flag or field to opt
into dependent mode, the processor inspects the operations and chooses the path. This
preserves full backward compatibility — a batch file that doesn't use `capture`,
`capture_append`, or `depends_on` runs exactly as before.

**Implicit dependency inference.** Operations that reference `{{variable}}` in their
args automatically depend on the operation(s) that capture that variable. This reduces
boilerplate: for simple linear chains, `depends_on` can be omitted entirely. For
`capture_append` variables with multiple providers, the consumer implicitly depends on
all of them.

**Atomic execution semantics.** In dependent mode, execution halts on the first
failure. Subsequent operations are marked as "Skipped due to prior failure" with no
HTTP requests made. This prevents cascading errors and gives agents a clear signal
about where the workflow broke. The alternative — continue-on-error — was considered
but rejected for the dependent path because downstream operations would fail anyway
due to missing captured variables.

**JQ for extraction.** Capture queries reuse the existing `apply_jq_filter` function
from the execution engine rather than introducing a new extraction syntax. JQ is
already a dependency and is well-understood by agents.

**Scalar/list variable separation.** `capture` produces scalar string values;
`capture_append` accumulates into lists that interpolate as JSON array literals.
Scalars take precedence when both exist for the same name. This keeps the common case
(scalar capture) simple while supporting fan-out/aggregate patterns without requiring
the user to manage array construction.

**Topological sort with original-order preservation.** Kahn's algorithm is used for
topological sorting. Among operations with equal topological rank (no ordering
constraint between them), the original file order is preserved. This makes execution
order predictable and debuggable.

## Consequences

### Positive

- Multi-step agent workflows execute in a single `aperture` invocation, eliminating
  per-step roundtrip latency.
- The batch file becomes a self-contained workflow specification that can be validated
  (cycle detection, missing references) before any HTTP request is made.
- Full backward compatibility — no changes to existing batch files or concurrent
  execution behavior.
- Structured error reporting for all dependency-related failures (cycles, missing
  references, undefined variables, capture failures) with `--json-errors` support.

### Negative

- Dependent execution is strictly sequential. There is no support for executing
  independent sub-graphs in parallel within a dependent batch. This is acceptable for
  the current use case but may become a limitation for complex workflows.
- `--dry-run` combined with dependent batches is of limited utility: the first
  operation's capture will fail (dry-run output doesn't match the real response
  schema), causing all subsequent operations to be skipped.
- The `{{variable}}` syntax uses double-braces which could conflict with literal
  `{{` in JSON bodies. This is mitigated by only scanning for variables when the
  batch actually uses dependency features (`has_dependencies()` check).

### Future considerations

- **Parallel sub-graph execution**: operations at the same topological level with no
  mutual dependencies could be executed concurrently. This would require tracking
  in-degree per level and coordinating capture writes.
- **Conditional execution**: skip or branch based on captured values (e.g., only add
  to group if user creation returned a specific status).
- **Per-operation retry in dependent mode**: currently, a failed operation halts the
  entire batch. Integrating the retry system could allow transient failures to be
  retried before declaring the operation failed.
