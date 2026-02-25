//! Dependency graph construction, validation, and topological sorting for batch operations.
//!
//! Builds a directed acyclic graph from `depends_on` edges and implicit variable
//! references, validates structure, detects cycles, and produces a topological
//! execution order via Kahn's algorithm.

use crate::batch::BatchOperation;
use crate::error::Error;
use std::collections::{HashMap, HashSet, VecDeque};

/// Result of dependency graph resolution: an ordered list of operation indices.
pub type ExecutionOrder = Vec<usize>;

/// Validates the batch operations and returns a topological execution order.
///
/// # Validation rules
///
/// 1. Every operation that uses `capture`, `capture_append`, or `depends_on` must have an `id`.
/// 2. Every `depends_on` reference must point to an existing operation `id`.
/// 3. Implicit dependencies from `{{variable}}` usage are inferred from `capture`/`capture_append`.
/// 4. The resulting dependency graph must be acyclic.
///
/// # Errors
///
/// Returns an error if any validation rule is violated or a cycle is detected.
pub fn resolve_execution_order(operations: &[BatchOperation]) -> Result<ExecutionOrder, Error> {
    validate_ids(operations)?;

    let id_to_index = build_id_index(operations)?;
    let capture_var_to_op = build_capture_index(operations, &id_to_index);

    let adjacency = build_adjacency(operations, &id_to_index, &capture_var_to_op)?;
    topological_sort(operations, &adjacency)
}

/// Checks whether the batch uses any dependency features.
///
/// Empty maps/vecs (e.g. `capture: {}`, `depends_on: []`) are treated as absent
/// so they don't accidentally trigger dependent mode.
#[must_use]
pub fn has_dependencies(operations: &[BatchOperation]) -> bool {
    operations.iter().any(|op| {
        op.depends_on.as_ref().is_some_and(|d| !d.is_empty())
            || op.capture.as_ref().is_some_and(|c| !c.is_empty())
            || op.capture_append.as_ref().is_some_and(|c| !c.is_empty())
    })
}

// ── Validation ──────────────────────────────────────────────────────

/// Ensures every operation that requires an `id` has one.
fn validate_ids(operations: &[BatchOperation]) -> Result<(), Error> {
    for (i, op) in operations.iter().enumerate() {
        let Some(context) = id_requirement_context(op) else {
            continue;
        };

        if op.id.is_none() {
            return Err(Error::batch_missing_id(format!(
                "operation at index {i} uses {context} but has no id"
            )));
        }
    }
    Ok(())
}

/// Returns the reason an operation needs an `id`, or `None` if it doesn't.
///
/// Only operations that *produce* captured values or declare explicit dependencies
/// require an ID. Operations that merely *consume* variables via `{{var}}` in args
/// do not need an ID — implicit dependency edges use the operation's index, and
/// omitting the ID requirement here avoids false positives when literal `{{`
/// appears in argument strings (e.g. JSON bodies with template syntax).
///
/// Empty maps/vecs are treated as absent, consistent with [`has_dependencies`].
fn id_requirement_context(op: &BatchOperation) -> Option<&'static str> {
    let has_capture = op.capture.as_ref().is_some_and(|c| !c.is_empty());
    let has_append = op.capture_append.as_ref().is_some_and(|c| !c.is_empty());
    if has_capture || has_append {
        return Some("capture");
    }
    if op.depends_on.as_ref().is_some_and(|d| !d.is_empty()) {
        return Some("depends_on");
    }
    None
}

/// Builds a map from operation `id` → index in the operations slice.
///
/// # Errors
///
/// Returns an error if two or more operations share the same `id`.
fn build_id_index(operations: &[BatchOperation]) -> Result<HashMap<&str, usize>, Error> {
    let mut map = HashMap::new();
    for (i, op) in operations.iter().enumerate() {
        let Some(id) = op.id.as_deref() else {
            continue;
        };
        if let Some(existing_idx) = map.insert(id, i) {
            return Err(Error::validation_error(format!(
                "Duplicate operation id '{id}': found at index {existing_idx} and {i}"
            )));
        }
    }
    Ok(map)
}

/// Builds a map from captured variable name → indices of all operations that capture it.
///
/// For `capture` (scalar) variables there is typically one provider.
/// For `capture_append` (list) variables there may be many — all must complete
/// before a consumer can safely interpolate the accumulated list.
fn build_capture_index<'a>(
    operations: &'a [BatchOperation],
    id_to_index: &HashMap<&'a str, usize>,
) -> HashMap<&'a str, Vec<usize>> {
    let mut map: HashMap<&str, Vec<usize>> = HashMap::new();
    for op in operations {
        let Some(id) = op.id.as_deref() else {
            continue;
        };
        let Some(&idx) = id_to_index.get(id) else {
            continue;
        };
        if let Some(captures) = &op.capture {
            for var_name in captures.keys() {
                map.entry(var_name.as_str()).or_default().push(idx);
            }
        }
        if let Some(appends) = &op.capture_append {
            for var_name in appends.keys() {
                map.entry(var_name.as_str()).or_default().push(idx);
            }
        }
    }
    map
}

/// Extracts variable names referenced in `{{name}}` patterns from a string.
fn extract_variable_references(s: &str) -> Vec<&str> {
    let mut vars = Vec::new();
    let mut remaining = s;
    while let Some(start) = remaining.find("{{") {
        let after_open = &remaining[start + 2..];
        let Some(end) = after_open.find("}}") else {
            break;
        };
        let var_name = &after_open[..end];
        if !var_name.is_empty() {
            vars.push(var_name);
        }
        remaining = &after_open[end + 2..];
    }
    vars
}

/// Builds the adjacency list (edges: dependency → dependent).
///
/// An edge from `a` to `b` means `a` must execute before `b`.
fn build_adjacency(
    operations: &[BatchOperation],
    id_to_index: &HashMap<&str, usize>,
    capture_var_to_op: &HashMap<&str, Vec<usize>>,
) -> Result<Vec<Vec<usize>>, Error> {
    let n = operations.len();
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];

    for (i, op) in operations.iter().enumerate() {
        let mut deps: HashSet<usize> = HashSet::new();

        // Explicit depends_on
        if let Some(dep_ids) = &op.depends_on {
            for dep_id in dep_ids {
                let &dep_idx = id_to_index.get(dep_id.as_str()).ok_or_else(|| {
                    Error::batch_missing_dependency(op.id.as_deref().unwrap_or("<unnamed>"), dep_id)
                })?;
                deps.insert(dep_idx);
            }
        }

        // Implicit dependencies from variable references in args.
        // For capture_append variables with multiple providers, this
        // correctly adds edges from ALL providers to the consumer.
        let implicit_deps = op
            .args
            .iter()
            .flat_map(|arg| extract_variable_references(arg))
            .filter_map(|var| capture_var_to_op.get(var))
            .flat_map(|indices| indices.iter().copied())
            .filter(|&idx| idx != i);
        deps.extend(implicit_deps);

        for dep_idx in deps {
            adj[dep_idx].push(i);
        }
    }

    Ok(adj)
}

/// Kahn's algorithm for topological sorting with cycle detection.
///
/// Returns indices in execution order. Operations with no dependencies
/// preserve their original relative order.
fn topological_sort(
    operations: &[BatchOperation],
    adj: &[Vec<usize>],
) -> Result<ExecutionOrder, Error> {
    let n = operations.len();
    let mut in_degree = vec![0usize; n];
    for successors in adj {
        for &succ in successors {
            in_degree[succ] += 1;
        }
    }

    // Seed queue with zero-in-degree nodes in original order
    let mut queue: VecDeque<usize> = (0..n).filter(|&i| in_degree[i] == 0).collect();

    let mut order = Vec::with_capacity(n);
    while let Some(node) = queue.pop_front() {
        order.push(node);
        // Sort successors to preserve original order among siblings
        let mut successors = adj[node].clone();
        successors.sort_unstable();
        for succ in successors {
            in_degree[succ] -= 1;
            if in_degree[succ] == 0 {
                queue.push_back(succ);
            }
        }
    }

    if order.len() != n {
        // Build the unresolved subgraph (nodes with in-degree > 0 after Kahn).
        // This includes cycle members and possibly downstream nodes blocked by
        // those cycles; we then extract one concrete cycle path from that subgraph.
        let unresolved: Vec<bool> = in_degree.iter().map(|&d| d > 0).collect();

        let cycle_indices = find_cycle_path(adj, &unresolved)
            .unwrap_or_else(|| (0..n).filter(|&i| unresolved[i]).collect());

        let cycle_ids: Vec<String> = cycle_indices
            .into_iter()
            .map(|i| {
                operations[i]
                    .id
                    .clone()
                    .unwrap_or_else(|| format!("index {i}"))
            })
            .collect();

        return Err(Error::batch_cycle_detected(&cycle_ids));
    }

    Ok(order)
}

/// Finds one concrete directed cycle in the unresolved subgraph.
///
/// Returns node indices describing the cycle path. The start node is repeated
/// at the end (e.g., `[a, b, a]`) to make the loop explicit in error messages.
fn find_cycle_path(adj: &[Vec<usize>], unresolved: &[bool]) -> Option<Vec<usize>> {
    let n = adj.len();
    let mut color = vec![0u8; n]; // 0 = unvisited, 1 = visiting, 2 = done
    let mut stack = Vec::new();
    let mut stack_pos: HashMap<usize, usize> = HashMap::new();

    for start in 0..n {
        if !unresolved[start] || color[start] != 0 {
            continue;
        }

        if let Some(cycle) = dfs_cycle(
            start,
            adj,
            unresolved,
            &mut color,
            &mut stack,
            &mut stack_pos,
        ) {
            return Some(cycle);
        }
    }

    None
}

fn dfs_cycle(
    node: usize,
    adj: &[Vec<usize>],
    unresolved: &[bool],
    color: &mut [u8],
    stack: &mut Vec<usize>,
    stack_pos: &mut HashMap<usize, usize>,
) -> Option<Vec<usize>> {
    color[node] = 1;
    stack_pos.insert(node, stack.len());
    stack.push(node);

    let mut successors = adj[node].clone();
    successors.sort_unstable();

    for succ in successors {
        if !unresolved[succ] {
            continue;
        }

        match color[succ] {
            0 => {
                if let Some(cycle) = dfs_cycle(succ, adj, unresolved, color, stack, stack_pos) {
                    return Some(cycle);
                }
            }
            1 => {
                // Back-edge to an ancestor in the DFS stack => concrete cycle.
                let start = *stack_pos
                    .get(&succ)
                    .expect("visiting node must be present in DFS stack position map");
                let mut cycle = stack[start..].to_vec();
                cycle.push(succ);
                return Some(cycle);
            }
            _ => {}
        }
    }

    stack.pop();
    stack_pos.remove(&node);
    color[node] = 2;
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::batch::BatchOperation;
    use std::collections::HashMap;

    fn op(id: &str) -> BatchOperation {
        BatchOperation {
            id: Some(id.to_string()),
            args: vec![],
            ..Default::default()
        }
    }

    fn op_with_deps(id: &str, deps: &[&str]) -> BatchOperation {
        BatchOperation {
            id: Some(id.to_string()),
            args: vec![],
            depends_on: Some(deps.iter().map(|s| (*s).to_string()).collect()),
            ..Default::default()
        }
    }

    fn op_with_capture(id: &str, captures: &[(&str, &str)]) -> BatchOperation {
        let mut map = HashMap::new();
        for &(k, v) in captures {
            map.insert(k.to_string(), v.to_string());
        }
        BatchOperation {
            id: Some(id.to_string()),
            args: vec![],
            capture: Some(map),
            ..Default::default()
        }
    }

    fn op_with_var_ref(id: &str, arg_template: &str) -> BatchOperation {
        BatchOperation {
            id: Some(id.to_string()),
            args: vec![arg_template.to_string()],
            ..Default::default()
        }
    }

    #[test]
    fn no_dependencies_preserves_original_order() {
        let ops = vec![op("a"), op("b"), op("c")];
        let order = resolve_execution_order(&ops).unwrap();
        assert_eq!(order, vec![0, 1, 2]);
    }

    #[test]
    fn explicit_linear_chain() {
        let ops = vec![
            op("create"),
            op_with_deps("get", &["create"]),
            op_with_deps("delete", &["get"]),
        ];
        let order = resolve_execution_order(&ops).unwrap();
        assert_eq!(order, vec![0, 1, 2]);
    }

    #[test]
    fn explicit_fan_in() {
        // a and b are independent; c depends on both
        let ops = vec![op("a"), op("b"), op_with_deps("c", &["a", "b"])];
        let order = resolve_execution_order(&ops).unwrap();
        // a and b before c
        assert!(
            order.iter().position(|&x| x == 0).unwrap()
                < order.iter().position(|&x| x == 2).unwrap()
        );
        assert!(
            order.iter().position(|&x| x == 1).unwrap()
                < order.iter().position(|&x| x == 2).unwrap()
        );
    }

    #[test]
    fn implicit_dependency_from_variable_ref() {
        let ops = vec![
            op_with_capture("create", &[("user_id", ".id")]),
            op_with_var_ref("get", "--user-id={{user_id}}"),
        ];
        let order = resolve_execution_order(&ops).unwrap();
        assert_eq!(order, vec![0, 1]);
    }

    #[test]
    fn cycle_detection_two_nodes() {
        let ops = vec![op_with_deps("a", &["b"]), op_with_deps("b", &["a"])];
        let result = resolve_execution_order(&ops);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("cycle"), "expected cycle error, got: {err}");
        // Error should reference operation IDs, not just indices
        assert!(
            err.contains('a') && err.contains('b'),
            "expected operation IDs in cycle error, got: {err}"
        );
    }

    #[test]
    fn cycle_detection_three_nodes() {
        let ops = vec![
            op_with_deps("a", &["c"]),
            op_with_deps("b", &["a"]),
            op_with_deps("c", &["b"]),
        ];
        let result = resolve_execution_order(&ops);
        assert!(result.is_err());
    }

    #[test]
    fn cycle_error_excludes_downstream_non_cycle_nodes() {
        // a <-> b is the cycle; c depends on a but is not part of the cycle.
        let ops = vec![
            op_with_deps("a", &["b"]),
            op_with_deps("b", &["a"]),
            op_with_deps("c", &["a"]),
        ];
        let result = resolve_execution_order(&ops);
        assert!(result.is_err());

        let err = result.unwrap_err().to_string();
        let reported_cycle: Vec<&str> = err
            .rsplit(':')
            .next()
            .unwrap_or_default()
            .split('→')
            .map(str::trim)
            .collect();

        assert!(
            reported_cycle.contains(&"a") && reported_cycle.contains(&"b"),
            "expected cycle members a and b, got: {err}"
        );
        assert!(
            !reported_cycle.contains(&"c"),
            "expected downstream node c to be excluded from cycle report, got: {err}"
        );
    }

    #[test]
    fn missing_dependency_reference() {
        let ops = vec![op("a"), op_with_deps("b", &["nonexistent"])];
        let result = resolve_execution_order(&ops);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("nonexistent"),
            "expected missing dep error, got: {err}"
        );
    }

    #[test]
    fn duplicate_ids_rejected() {
        let ops = vec![op("dup"), op("dup")];
        let result = resolve_execution_order(&ops);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Duplicate operation id 'dup'"),
            "expected duplicate id error, got: {err}"
        );
    }

    #[test]
    fn missing_id_on_capture_operation() {
        let op = BatchOperation {
            capture: Some(HashMap::from([("x".into(), ".id".into())])),
            ..Default::default()
        };
        let ops = vec![op];
        let result = resolve_execution_order(&ops);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("no id"),
            "expected missing id error, got: {err}"
        );
    }

    #[test]
    fn missing_id_on_depends_on_operation() {
        let op = BatchOperation {
            depends_on: Some(vec!["other".into()]),
            ..Default::default()
        };
        let ops = vec![op];
        let result = resolve_execution_order(&ops);
        assert!(result.is_err());
    }

    #[test]
    fn variable_consumer_without_id_is_valid() {
        // An operation that references {{var}} without an id should pass
        // validation — only producers (capture/capture_append) and explicit
        // depends_on users need IDs.
        let ops = vec![
            op_with_capture("producer", &[("uid", ".id")]),
            BatchOperation {
                args: vec!["--id".into(), "{{uid}}".into()],
                ..Default::default()
            },
        ];
        let order = resolve_execution_order(&ops).unwrap();
        // Producer before consumer
        assert!(
            order.iter().position(|&x| x == 0).unwrap()
                < order.iter().position(|&x| x == 1).unwrap()
        );
    }

    #[test]
    fn has_dependencies_returns_false_for_simple_batch() {
        let ops = vec![op("a"), op("b")];
        assert!(!has_dependencies(&ops));
    }

    #[test]
    fn has_dependencies_returns_true_for_capture() {
        let ops = vec![op_with_capture("a", &[("x", ".id")])];
        assert!(has_dependencies(&ops));
    }

    #[test]
    fn has_dependencies_returns_true_for_depends_on() {
        let ops = vec![op_with_deps("a", &["b"])];
        assert!(has_dependencies(&ops));
    }

    #[test]
    fn has_dependencies_returns_false_for_bare_variable_ref() {
        // A {{var}} reference without any capture/capture_append/depends_on
        // should NOT trigger the dependent path — preserves backward compat
        // for literal {{ in arguments.
        let ops = vec![op_with_var_ref("a", "{{some_var}}")];
        assert!(!has_dependencies(&ops));
    }

    #[test]
    fn has_dependencies_returns_false_for_empty_maps_and_vecs() {
        // Empty capture/capture_append/depends_on should not trigger dependent mode.
        // This prevents confusing "missing id" errors when users write e.g. `capture: {}`.
        let ops = vec![BatchOperation {
            id: Some("a".into()),
            args: vec![],
            capture: Some(HashMap::new()),
            capture_append: Some(HashMap::new()),
            depends_on: Some(vec![]),
            ..Default::default()
        }];
        assert!(!has_dependencies(&ops));
    }

    #[test]
    fn empty_capture_does_not_require_id() {
        // An operation with capture: {} (empty map) should not require an id.
        let ops = vec![BatchOperation {
            args: vec![],
            capture: Some(HashMap::new()),
            ..Default::default()
        }];
        // Should succeed — empty capture is treated as absent
        let order = resolve_execution_order(&ops).unwrap();
        assert_eq!(order, vec![0]);
    }

    #[test]
    fn has_dependencies_returns_true_for_variable_ref_with_capture_provider() {
        let ops = vec![
            op_with_capture("producer", &[("some_var", ".id")]),
            op_with_var_ref("consumer", "{{some_var}}"),
        ];
        assert!(has_dependencies(&ops));
    }

    #[test]
    fn extract_variable_references_basic() {
        let vars = extract_variable_references("--id={{user_id}}");
        assert_eq!(vars, vec!["user_id"]);
    }

    #[test]
    fn extract_variable_references_multiple() {
        let vars = extract_variable_references("{{a}} and {{b}}");
        assert_eq!(vars, vec!["a", "b"]);
    }

    #[test]
    fn extract_variable_references_none() {
        let vars = extract_variable_references("no variables here");
        assert!(vars.is_empty());
    }

    #[test]
    fn extract_variable_references_unclosed() {
        let vars = extract_variable_references("{{unclosed");
        assert!(vars.is_empty());
    }

    #[test]
    fn capture_append_creates_implicit_dependency() {
        let append_op = BatchOperation {
            id: Some("beat-1".into()),
            args: vec![],
            capture_append: Some(HashMap::from([("ids".into(), ".id".into())])),
            ..Default::default()
        };
        let consumer = op_with_var_ref("final", "{{ids}}");
        let ops = vec![append_op, consumer];
        let order = resolve_execution_order(&ops).unwrap();
        assert_eq!(order, vec![0, 1]);
    }

    #[test]
    fn capture_append_multiple_providers_all_become_implicit_deps() {
        // Two providers capture_append into the same list; consumer references
        // {{ids}} without explicit depends_on. Both providers must run first.
        let beat_1 = BatchOperation {
            id: Some("beat-1".into()),
            args: vec![],
            capture_append: Some(HashMap::from([("ids".into(), ".id".into())])),
            ..Default::default()
        };
        let beat_2 = BatchOperation {
            id: Some("beat-2".into()),
            args: vec![],
            capture_append: Some(HashMap::from([("ids".into(), ".id".into())])),
            ..Default::default()
        };
        let consumer = op_with_var_ref("aggregate", "{{ids}}");
        let ops = vec![beat_1, beat_2, consumer];
        let order = resolve_execution_order(&ops).unwrap();
        let pos = |idx: usize| order.iter().position(|&x| x == idx).unwrap();
        // Both providers must appear before the consumer
        assert!(pos(0) < pos(2), "beat-1 should precede aggregate");
        assert!(pos(1) < pos(2), "beat-2 should precede aggregate");
    }

    #[test]
    fn diamond_dependency() {
        // a -> b, a -> c, b -> d, c -> d
        let ops = vec![
            op("a"),
            op_with_deps("b", &["a"]),
            op_with_deps("c", &["a"]),
            op_with_deps("d", &["b", "c"]),
        ];
        let order = resolve_execution_order(&ops).unwrap();
        let pos = |id: usize| order.iter().position(|&x| x == id).unwrap();
        assert!(pos(0) < pos(1));
        assert!(pos(0) < pos(2));
        assert!(pos(1) < pos(3));
        assert!(pos(2) < pos(3));
    }
}
