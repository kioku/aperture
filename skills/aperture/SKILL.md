---
name: aperture
description: Use Aperture for API discovery, multi-call workflows, local conventions, troubleshooting, and safe execution.
---
# Aperture API workflows

Aperture turns OpenAPI specifications into discoverable API commands and reusable multi-call workflows. Activate this skill when using Aperture, discovering an API's capabilities, constructing API requests, processing paginated results, troubleshooting calls, or following local workflow conventions.

1. Read `aperture skills get core --full` before constructing requests. The binary-distributed handbook introduces Aperture and teaches discovery, configuration/authentication, request bodies, output/errors, pagination, batches, retries, caching, workflow skills, and safe verification. It matches the running package and needs no registry or download.
2. Run `aperture skills list --json` to discover relevant local workflows, then read `aperture skills get <name> --full` and its needed references.
3. Inspect current help and capability schemas for exact operation names, flags, and API requirements. Follow the operator's scope; skill activation never grants mutation or credential permissions.

If Aperture is unavailable, report the missing tool rather than installing packages or guessing API commands. This stable entry point delegates version-specific instructions to the running binary, avoiding stale copies of the handbook.
