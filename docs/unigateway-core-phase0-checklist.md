# UniGateway Core Phase 0 Checklist

Status: Confirmed

Date: 2026-04-06

Confirmed on: 2026-04-06

Related documents:

- [docs/unigateway-core-api-draft.md](docs/unigateway-core-api-draft.md)
- [docs/unigateway-core-implementation-plan.md](docs/unigateway-core-implementation-plan.md)

## 1. Purpose

This checklist defines the minimum set of decisions that must be confirmed before implementation of `unigateway-core` begins.

Phase 0 is complete only when all required items below are explicitly confirmed.

## 2. Decision Checklist

| ID | Decision | Current recommendation | Why it matters | Status |
| --- | --- | --- | --- | --- |
| P0-1 | Introduce `unigateway-core` as a new reusable crate | Yes | Prevents continued coupling between product code and core execution logic | Confirmed |
| P0-2 | Keep the current `unigateway` package as the product-facing binary/package | Yes | Minimizes disruption to existing users and release flow | Confirmed |
| P0-3 | Keep `unigateway-core` in the same repository and workspace during the first migration | Yes | Simplifies extraction, refactor review, and shared CI during early phases | Confirmed |
| P0-4 | Core crate v1 scope includes `chat`, `responses`, and `embeddings` only | Yes | Prevents scope drift during extraction | Confirmed |
| P0-5 | Core crate supports only built-in `OpenAiCompatible` and `Anthropic` drivers in v1 | Yes | Keeps protocol surface narrow and stable | Confirmed |
| P0-6 | `unigateway-core` must not depend on `llm-connector` | Yes | Prevents third-party abstraction leakage into the core API | Confirmed |
| P0-7 | Vendor-specific quirks should be normalized in upper layers whenever possible | Yes | Keeps the core crate protocol surface clean | Confirmed |
| P0-8 | Custom provider support should be implemented through plugin drivers, not special cases in core | Yes | Preserves extensibility without polluting built-in behavior | Confirmed |
| P0-9 | Core runtime state must be pure in-memory with snapshot semantics | Yes | Required for zero-database embedding and hot updates | Confirmed |
| P0-10 | Product-layer auth, quota, admin APIs, config persistence, CLI, and MCP remain outside core | Yes | Maintains the core/product boundary defined by the RFC | Confirmed |
| P0-11 | v1 built-in routing strategies are `Random` and `RoundRobin` only | Yes | Prevents over-design before the engine boundary is stable | Confirmed |
| P0-12 | Retry surface is limited to `429`, `5xx`, timeout, and transport failures | Yes | Keeps failover policy predictable in v1 | Confirmed |
| P0-13 | Streaming uses dual-return semantics | Yes | Required for usage collection after stream completion | Confirmed |
| P0-14 | Transparent retry stops after first downstream stream event | Yes | Prevents broken or duplicated downstream streams | Confirmed |
| P0-15 | Result reports expose `selected_endpoint_id`, never raw upstream keys | Yes | Preserves observability without secret leakage | Confirmed |

## 3. Hard Blockers

Implementation should not begin until the following blocker decisions are fully confirmed:

### 3.1 Package Boundary

Must confirm:

- whether `unigateway-core` is introduced as a new crate now
- whether the current `unigateway` crate remains the product-facing package
- whether both remain in the same repository during migration

Reason:

- this affects Cargo layout, module movement, CI, tests, and release flow

### 3.2 Transport Boundary

Must confirm:

- `llm-connector` is removed from the core direction
- core-owned built-in drivers are the default path
- custom drivers are the only supported escape hatch for non-standard providers

Reason:

- this affects every protocol and streaming implementation decision

### 3.3 Product/Core Separation

Must confirm:

- auth and quota remain in the product layer
- config persistence remains in the product layer
- admin CRUD remains in the product layer
- CLI and MCP remain in the product layer

Reason:

- without this boundary, extraction work will drift back into a mixed design

### 3.4 v1 Capability Scope

Must confirm:

- `chat`
- `responses`
- `embeddings`

Reason:

- this defines the minimum acceptable coverage for Phase 3 and Phase 4

## 4. Recommended Confirmation Format

The fastest way to finish Phase 0 is to reply with one line per item using this format:

```text
P0-1 yes
P0-2 yes
...
P0-15 yes
```

If any item should be changed, use:

```text
P0-6 change: keep llm-connector as optional external adapter crate only
```

## 5. Phase 0 Exit Criteria

Phase 0 is considered complete only when all of the following are true:

1. The crate boundary is confirmed.
2. The transport and plugin boundary is confirmed.
3. The product/core responsibility split is confirmed.
4. The v1 capability scope is confirmed.
5. No blocker remains that would force redesign in Phase 1 or Phase 2.

Current status: complete.

## 6. Recommended Next Step After Confirmation

Once the checklist is confirmed, the next implementation planning action should be:

1. create a Phase 1 task sheet for core-native type extraction
2. define the initial file/module skeleton for `unigateway-core`
3. identify the first adapter layer from current types into core-owned types

Next document: [docs/unigateway-core-phase1-task-sheet.md](docs/unigateway-core-phase1-task-sheet.md)
