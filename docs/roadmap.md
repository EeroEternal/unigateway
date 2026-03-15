# UniGateway Roadmap

This document guides the next few rounds of iterations, aiming to converge UniGateway from a "general-purpose lightweight LLM gateway" into a "unified model entry point for individual developers / AI power users."

## 0. Current Progress Update (2026-03-15)

The main target has entered the phase of "core capabilities are shaped, continuing with tool integration and product polishing."

### Completed Core Capabilities

- Mode-oriented CLI: `ug mode list/show/use`
- Route explanation: `ug route explain`
- Integration template output: `ug integrations`
- Diagnostics and smoke tests: `ug doctor`, `ug test`
- Quickstart defaults to generating `fast` / `strong` / `backup`
- Provider / model data changed to dynamically generated based on registry
- OpenAI / Anthropic default to enabling streaming

### Major Completed Refactors

- `cli.rs` split into `modes / render / quickstart / diagnostics`
- `main.rs` quickstart/setup logic split into `setup/`
- `gateway.rs` split into `gateway/chat.rs`, `gateway/streaming.rs`
- `protocol.rs` split into `protocol/client.rs`, `protocol/messages.rs`
- `config.rs` split into `config/schema.rs`, `store.rs`, `select.rs`, `admin.rs`
- `cli/render.rs` split into `cli/render/integrations.rs`, `cli/render/routes.rs`

### Current Tool Support Status

Tools with templates or explicit support:

- Cursor
- Claude Code
- Codex
- OpenClaw
- Zed
- Droid
- OpenCode
- Generic `env / python / node / curl / anthropic`

### Current Integration Priority

Subsequent product polishing will prioritize the following set of tools:

1. OpenClaw
2. Zed
3. Claude Code
4. Cursor
5. Droid
6. OpenCode

### Current Assessment

The core direction of the project—"unified local entry point + mode abstraction + multi-upstream routing + tool integration"—has been realized. The next focus is not large-scale internal rewrite, but:

- Making high-priority tools into first-class citizenship integrations with lower friction
- Strengthening multi-mode workflows
- Continuing to improve diagnostics, explanation, and default experience

## 1. Overall Development Goal

In subsequent development, we will only push forward around one core goal:

**Allowing developers who use multiple AI tools, multiple models, and multiple providers simultaneously to gain unified access, stable switching, and clear diagnostic capabilities with minimum configuration cost.**

This means our priority ordering should be:

1. Mastering the first high-frequency scenario
2. Supplementing capabilities that support this scenario
3. Temporarily stopping expansion directions that disperse attention

## 2. Recent Development Principles

### 2.1 Feature Additions Must Serve the Main Scenario

All future new features must first ask one question:

**Does it directly help individual developers connect faster, use more stably, switch more easily, or diagnose more readily?**

If not, it should not enter the recent development plan.

### 2.2 Prioritize "User Actions," Not "Low-level Resource CRUD"

Recent design and implementation should revolve around these actions:

- Integrating a tool
- Configuring a common mode
- Testing if a mode is available
- Explaining where a mode goes
- Recovering quickly during provider failure

### 2.3 Prioritize Solutions That Can Be Used by Default

Compared to complex configurability, individual developers need:

- Default modes
- Default fallback
- Default provider templates
- Default verification methods

## 3. Breakdown of Product Phases

It is suggested to break the development plan into four phases.

## Phase 0: Direction Convergence and Model Renaming

### Goal

First converge the product expression, conceptual layer, and command organization to avoid changing while doing later.

### Deliverables

- Clear external product language: `mode`, `upstream`, `integration`, `doctor`
- Keep internal `service/provider/binding`, but no longer treat them as the main narrative
- Form a unified command naming direction
- Clear list of short-term non-goals

### Implementation Focus

- Streamline the CLI top-level command structure
- Define mapping between modes and existing services
- Define mapping between upstream profiles and existing providers
- Confirm the new user path for quickstart

### Acceptance Criteria

- Internal team can discuss requirements and implementation using unified vocabulary
- Subsequent documents, commands, and implementations no longer mix multiple sets of concepts

## Phase 1: First User Experience Loop

### Goal

Make the process from installation to first tool integration success short enough and stable enough for users.

### Core Problem

The current project already has gateway and routing capabilities, but the "first success experience" is not yet tailored enough for AI power users, requiring a refactoring of the quickstart and tool integration paths.

### Deliverables

- Remake `ug quickstart`
- Automatically generate default modes: `fast`, `strong`, `backup`
- Provide access snippet output for mainstream tools
- Provide an immediately verifiable test command

### Suggested Command Directions

- `ug quickstart`
- `ug integrations`
- `ug test`

### Code Work Items

- `main.rs`: Adjust command organization and help text
- `cli.rs`: Rewrite quickstart logic to generate configuration around modes
- `config.rs`: Add mode semantic layer
- `provider-examples` relevant logic: Consolidate as reusable provider presets

### Acceptance Criteria

- New users can complete installation, configuration, startup, and verification within minutes
- At least one AI tool and one script call can connect smoothly
- Users do not need to understand `service/provider/binding` first

## Phase 2: Mode System and Explainable Routing

### Goal

Truly make "Mode" the core abstraction of the product, not just a configuration alias.

### Deliverables

- Mode list and details
- Primary and fallback upstreams corresponding to modes
- Route explanation capability
- Clearer default model mapping and provider selection logic

### Suggested Command Directions

- `ug mode list`
- `ug mode show <name>`
- `ug route explain <mode>`
- `ug mode use <name>`

### Code Work Items

- `config.rs`: Extend mode configuration structure
- `routing.rs`: Support clearer preference/fallback explanation
- `gateway.rs`: Integrate mode resolution into the request lifecycle
- `types.rs`: Supplement shared types oriented towards modes

### Acceptance Criteria

- Users can clearly know which upstreams a certain mode goes to
- Mode switching doesn't require direct editing of complex low-level configurations
- Fallback behavior is consistently explainable in both documentation and runtime

## Phase 3: Diagnostics, Reliability, and Daily Maintenance Experience

### Goal

Let users, in daily use, not only "be able to use" but "quickly know why when something goes wrong."

### Deliverables

- `ug doctor`
- `ug recent`
- Provider connectivity check
- Mode-level health status
- Recent error summary

### Suggested Command Directions

- `ug doctor`
- `ug recent`
- `ug doctor --provider <name>`
- `ug test <mode>`

### Code Work Items

- `system.rs`: Extend status information output
- `gateway.rs`: Supplement lightweight request logging and failure summary
- `config.rs` or new module: Maintain latest status snapshots
- `cli.rs`: Provide user-friendly diagnostic output

### Acceptance Criteria

- Users can distinguish whether it is a local configuration issue, gateway issue, or upstream issue
- Common failures can be initially located without flipping through logs
- Route and diagnostic outputs are friendly even to non-infrastructure engineers

## Phase 4: Stability Polishing and Product Completeness Enhancement

### Goal

After the core experience loop is established, complement the stability and boundary capabilities needed for long-term use.

### Deliverables

- Safer default configurations
- More reliable error prompts and recovery suggestions
- More stable configuration evolution schemes
- More systematic test coverage

### Priority Directions

- Tighten default admin security
- Explain local security boundaries for provider keys / gateway keys
- configuration migration and compatibility
- Strengthen tests related to fallback, model mapping, and diagnostics

### Acceptance Criteria

- Clear migration paths for old and new configurations
- Automated tests cover common core processes
- Default behavior fits local developer scenarios better

## 4. Explicit Directions to Pause or Postpone

To ensure focus, the following directions should not preempt main line resources in the short term:

- Heavy Web UI
- Enterprise-grade RBAC
- Multi-tenant billing system
- Complex scheduling algorithms
- Generalized SDK platform route

These directions are not "never to be done," but "not done now."

## 5. Suggested Recent Implementation Order

If proceeding with a minimum viable product rhythm, it is suggested to land in the following order:

1. Define mode semantics and CLI naming
2. Remake quickstart
3. Output tool integration templates
4. Add `ug test` and minimum validation path
5. Implement `mode list/show` and `route explain`
6. Implement `ug doctor`
7. Supplement status snapshots and latest error summaries
8. Finally handle deeper stability and configuration evolution

The core reason for this order is:

- First open user entry
- Then let users understand system behavior
- Then let users resolve failures
- Finally expand capability boundaries

## 6. Recommended Technical Breakdown

To avoid a one-time major change, it is suggested to push forward in a "semantic layer first, storage layer later" manner.

### Level 1: Semantic Layer Modification

First introduce in CLI and configuration interpretation layer:

- mode
- upstream profile
- integration template
- doctor snapshot

The underlying still reuses the current `service/provider/binding`.

### Level 2: Routing Layer Modification

In `routing.rs`, organize current general-purpose routing capabilities into something that fits an individual user's mental model:

- preference order
- fallback chain
- route explanation

### Level 3: Diagnostic Layer Supplementation

Without introducing heavy dependencies, add:

- Recent request summary
- Recent failure cause
- Provider connectivity probing
- Mode health snapshot

### Level 4: Storage Layer Evolution

After the semantic layer is stable, decide whether to formally migrate the configuration structure to the `modes/upstreams` semantic model.

## 7. Suggested Work Division for Code Modules

### `main.rs`

- Adjust command tree
- Strengthen help information and default entry narrative

### `cli.rs`

- Carry quickstart
- Carry mode management commands
- Carry tool integration output
- Carry doctor / test / route explain

### `config.rs`

- Extend mode semantics
- Carry upstream profile interpretation logic
- Maintain lightweight status snapshots

### `routing.rs`

- mode -> upstream chain resolution
- Fallback explanation output
- Simplify default routing decisions

### `gateway.rs`

- Resolve route by mode during request
- Write lightweight request result information
- Provide runtime data sources for diagnostic commands

### `system.rs`

- Keep health and metrics
- Add more user-oriented status output capabilities

## 8. Suggested Test Strategy

Under this direction, test focus should also be adjusted.

### Tests to Complement First

1. Whether configuration generated by quickstart is correct
2. Whether mapping from mode to routing chain is correct
3. Whether fallback order meets expectations
4. Whether route explanation output is correct
5. Whether diagnostic commands cover common failures

### Tests That Are Not a Priority

- Peripheral management CRUD scenarios
- Details of extended providers outside the current main line

## 9. Success Criteria

When the following conditions are met, it indicates this direction is starting to take hold:

- New users can quickly connect at least one AI tool
- Users can complete daily switching using modes instead of low-level resource concepts
- Users can quickly locate and recover during provider issues
- Documentation, CLI, and runtime output all revolve around a unified mental model

## 10. Conclusion

The subsequent development is not about continuing to push UniGateway toward a "larger and more comprehensive gateway platform," but making it a truly useful product for individual developers:

- Fast access
- Fast switching
- Stable fallback
- Clear diagnostics

Once this path is mastered, regardless of whether it expands to team scenarios in the future, it will be built on a more solid product foundation.
