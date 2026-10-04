---
name: release-workflow
description: Plan and verify an approved release using locally discovered API operations.
aperture:
  required_apis: [source, tracker]
---
# Release workflow

1. Read `aperture skills get core --full`; inspect `aperture skills list --json` for missing local API configurations. Stop if the required APIs are absent; configuration is not proof of credentials or health.
2. Discover current operations and flags using `aperture api source --help`, `aperture api source --describe-json`, and the equivalent commands for tracker. Select documented read operations to inspect release state, tests, and work items.
3. Present the exact release target and planned mutations to the operator. Obtain explicit approval before publication or status changes. Do not infer approval from this skill.
4. Construct a dry run of each discovered mutation using its documented flags and `--dry-run`. Check targets and payloads against approval. Execute only the approved calls; inspect every result and avoid retries after uncertain outcomes.
5. Verify publication and work-item state using the discovered read operations. Report evidence and any incomplete steps without exposing credentials.
