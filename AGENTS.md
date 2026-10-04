# Sailry engineering rules

## New-session bootstrap

- This file is the entry point for a new AI session. Read `ARCHITECTURE.md` before changing code.
- Start by running `git status --short` and `git log --oneline -5`. Git commits and verified test output are the delivery authority; prior chat claims are not.
- Determine the active task from the user's latest instructions, committed implementation, and verified results.
- Preserve every pre-existing uncommitted file until its ownership and purpose are understood; audit it rather than treating it as generated output or delivered work.
- External reference worktrees are read-only unless the user explicitly authorizes changes. Use reviewed, committed source as the reference; never import mixed working-tree experiments wholesale.
- When current library APIs are needed, follow the repository's Context7 instructions and verify examples against the exact pinned dependency revision.

## Scope and sequence

- Before implementation, map the change to the user's authorized task. Complete that task against its stated acceptance criteria. Instructions to continue or finish all tasks preserve this scope. Unlisted work must be a demonstrated prerequisite or blocking defect for the active task; optional improvements, speculative hardening, historical discoveries, and later-phase work must not become new completion gates.
- Usage statistics cover only Sailry's own sessions, including shared Client aggregation across Sailry Nodes. Do not collect third-party tool histories (such as Codex or Claude Code), external account quotas, or their billing data.

- Maintain the new product in this repository. Do not depend on a separately maintained Sailry Platform repository.
- Desktop uses `gpui-kit`; Agent execution stays on ADK-Rust. Do not introduce another desktop stack or Agent engine without an explicitly authorized architecture change.
- Host is a long-running headless service that Desktop and Mobile connect to, not an interactive CLI Agent. Its command-line entry starts/configures the service; conversations, Agent tasks, and development tools are controlled through the shared protocol, without a separate prompt loop or TUI.
- Preview data must be clearly identified and must not trigger models, tools, network pairing, terminals, or user-data mutations.
- Mobile is a Flutter controller. Preserve its Rust dependency boundary; a desktop-only Rust client test is not mobile acceptance. Do not expand an authorized desktop task into Mobile product work.
- Features use the same local and remote command/subscription boundary and require evidence on both paths for changed shared behavior.
- Inspect and reuse suitable committed source and tests when migrating behavior. Normalize ownership and naming, remove redundant adapters, and preserve provenance. Do not assume every component needs rewriting or can be imported unchanged.
- Follow the current architecture and accepted UI. Public architecture belongs in root `ARCHITECTURE.md`; coding and test conventions belong here and in `CONTRIBUTING.md`. Internal plans, reports and `docs/` are not public source. Update documentation only when requested or required by an authorized behavior change.

## Stage delivery

- Use an existing task as the delivery unit. Complete its cohesive implementation before final validation; do not create a separate build/test cycle for each field, helper, or small UI change. Keep commits reviewable without fragmenting acceptance.
- Once the task's stated acceptance criteria and relevant checks pass, self-review, commit, and advance to the next authorized task. Do not add completion gates or reopen finished work without a concrete change, failure, or evidence gap. Report the commit hash and actual verification results to the user.
- Choose validation from the actual change and affected behavior or dependency boundaries. Reuse existing tests and fixtures; add coverage for changed behavior or a demonstrated defect. Broaden beyond the relevant checks only for a concrete uncovered risk or failure. A stage boundary alone does not require workspace-wide tests, all-target builds, or every Mobile contract. Required local/remote and affected UI acceptance still apply. Changes limited to prose instructions need diff review, not product builds or tests.
- Reuse passing results while the code and validation inputs relevant to those results remain unchanged. Rerun after an affected implementation, dependency, configuration, or environment change, a relevant failure, or evidence invalidating the earlier result. Diagnose failed checks and verify the correction; do not weaken assertions or mark unverified behavior complete to reduce testing.
- Keep feature-stage commits small enough to review; never leave an entire reconstruction as an uncommitted working tree.
- A compile pass, model response, or mocked transport test proves only its own scope. Do not report full integration or performance acceptance from narrower evidence.
- Stage only files owned by the current task. Preserve unrelated changes. Commit locally unless the user explicitly requests a push or PR.
- Reconcile the active task with committed implementation and verified results when resuming. Keep delivery evidence in tests and Git; do not append reports, test transcripts, or commit ledgers to docs.

## Code and dependency boundaries

- Before the first product release, all Sailry-owned protocol, wire, schema, storage-format, and extension-contract versions stay at `v1` (numeric fields use `1`). Change the current definition in place; do not increment versions per feature or add legacy migrations, backward-compatibility branches, fallback decoders, or old-format acceptance tests. Keep third-party dependency versions and ordinary data revisions, event sequences, and request IDs unchanged. Never automatically convert, reset, or delete an existing profile to accommodate a development schema change; use isolated fresh test profiles and report incompatible existing data without modifying it. Introduce release compatibility only after an explicit product release decision.
- Write all source code, identifiers, comments, doc comments, test names, logs, diagnostics, developer-facing errors and public documentation in English. Chinese or other product-visible languages must exist only in i18n resources and be referenced by stable localization keys.
- Let crate, module, type, `impl`, and test-module scopes provide context. Keep function, type, trait, variable, and test names concise and precise; do not repeat the full module/type/path in a name or create sentence-length identifiers. Avoid obscure abbreviations and one-letter names except conventional iterators or coordinates.
- Group tests by the unit or behavior under test so individual test names describe only the distinguishing behavior. Prefer `mod panel_visibility { fn preserves_manual_choice() }` over a single globally overqualified test name.
- Follow the mandatory GPUI Kit reuse rules below; product layout requirements do not authorize replacing framework controls or its design system.
- Use GPUI Kit's re-exported GPUI; do not independently select a conflicting GPUI revision.
- Keep presentation code out of Protocol, Link, Client, and Node Runtime. Client owns UI-independent event reduction, ordering, deduplication, and snapshot recovery; GPUI and Flutter own layout, focus, drafts, selection, and expansion. Do not implement separate conversation state machines in each UI.
- Desktop and Host use the same Node bootstrap and shutdown implementation. One data profile has one running Node owner and one Link identity/endpoint; local Client access reuses them rather than starting a second runtime. Mobile carries only Client/Link and a thin FFI adapter, not Node, ADK, PTY, Git execution, database-tool drivers, or GPUI. Client-side identity and cache storage are allowed; they are not another business database.
- The execution Node owns effective session configuration and immutable sent-turn revisions. Another client resumes that configuration; controller defaults apply to new sessions, and changing an existing session requires an explicit revision-checked command. Keep credentials in the execution Node's protected storage, not events or UI caches.
- ADK is the Agent execution engine. Node integrates its persistence through one authoritative session history path; UI projections are rebuildable derivatives, not another runner or history writer.
- A durable command must reach durable Node admission before Link acknowledges safe receipt. Keep transport receipt distinct from business completion; reuse stable request IDs and report uncertain side effects without automatic replay. Do not add a second durable inbox by default.
- Desktop does not use Flutter Rust Bridge. The Mobile bridge only adapts shared Rust client contracts, handles and async subscriptions; it must not copy business rules or become another persistence layer.
- Keep feature modules inside their owning crate. Create another crate only for a demonstrated dependency, binary, platform, or testing boundary.
- Pin external dependencies and commit applicable lockfiles. ADK dependency selection belongs in the root Cargo workspace, not absolute developer-machine paths.
- Do not add a JavaScript runtime merely because the UI framework offers scripting. The application Shell layout is unrelated to enabling `gpui-shell`.

## File organization and design

- Organize code by feature and cohesive responsibility. Do not accumulate independent components, classes, types, handlers, and services in one file. Keep application entry points and Shell composition thin; do not turn `main.rs`, `shell.rs`, or `pages.rs` into catch-all implementations.
- Extract substantial components and independently changing responsibilities into focused modules within their owning crate. Keep small, tightly related types and private helpers together; do not enforce one file per type/function or split solely to meet an arbitrary line limit.
- Follow the architecture's ownership boundaries when splitting files: UI composition and presentation state stay in UI modules; execution, persistence, and transport stay with their respective backend owners. Moving mixed responsibilities into several files does not by itself fix the boundary.
- Prefer composition, explicit dependencies, and small interfaces over inheritance hierarchies or global mutable access. Modules expose only what consumers need and must not introduce circular dependencies or duplicate state ownership.
- Use design patterns to solve demonstrated problems: adapters at external or platform boundaries, strategies for genuinely interchangeable behavior, and explicit state transitions for complex lifecycles. Reuse the existing owner and framework mechanisms; do not create parallel state machines or require a pattern for every component.
- Avoid speculative base classes, one-to-one wrappers, pass-through service layers, generic utility dumping grounds, and traits or factories without a concrete substitution or testing need. Choose the simplest structure that keeps responsibilities clear and changes testable.
- Keep focused tests near the behavior they cover; move large test suites and fixtures into dedicated test modules. During review, check file cohesion, dependency direction, interface size, and duplicated logic. Refactor affected responsibilities with behavior-preserving tests, without expanding into unrelated rewrites.

## Practical safety and portability

- Prefer standard-library and maintained cross-platform crate APIs for paths, files, directories, and other OS resources. Keep business workflows shared across platforms; add a small platform adapter only for a verified dependency gap, never duplicate an entire feature for Unix and Windows.
- Keep safeguards proportional to concrete risks: authenticated resource access, path confinement, credential protection, ordinary revision-conflict detection, and confirmation for destructive actions. Do not add redundant approvals, speculative permission systems, or repeated confirmations to already authorized operations.
- Do not make optional hardening or platform-specific atomic exchange/rollback a prerequisite for ordinary editing and saving. Use a shared library-backed save path, preserve drafts on failure, and report actual conflicts or uncertain outcomes; do not claim database-style isolation from uncooperative external writers.
- Simplification must preserve necessary access boundaries and honest failure reporting. Missing platform integration is an implementation task, not an invented security restriction; distinguish library support, compilation, and actual platform acceptance.
- Literal non-English text is allowed in fixtures specifically testing Unicode paths, input, or content. Keep test names, comments, diagnostics, and developer-facing messages in English; do not obscure intentional language fixtures with Unicode escapes.

## Open-ended texting: minimal product copy

- Apply this rule to all application-authored, user-visible text: navigation, titles, buttons, menus, placeholders, tooltips, empty states, notifications, status, errors, and confirmations, including plugin-contributed UI. Keep copy concise, natural, and open-ended.
- Say only what the user needs at that moment. Prefer a short label or one clear sentence; remove filler, repeated context, obvious instructions, promotional language, and implementation explanations. Do not add descriptions or tooltips that merely repeat the label.
- Name actions directly, such as "Save", "Connect", or "Retry". Use consistent terms for the same action or resource. Omit terminal punctuation from short UI copy by default, including titles, labels, buttons, prompts, tooltips, and status text. Retain necessary punctuation between sentences in multi-sentence explanations and within meaningful syntax; do not alter code, paths, or user-authored content. Avoid arbitrary character limits that damage clarity or localization.
- Keep task prompts open to the user's intent. Use invitations such as "Describe your task" rather than prescribing a workflow, assuming a task, or listing every available capability. Specific forms should still name the information they require.
- Report status and outcomes plainly. Errors should state what happened and, when useful, one next action. Preserve essential distinctions such as unsaved changes, conflicts, unavailable features, and uncertain outcomes; brevity must not conceal risk or imply success.
- Use toast notifications as the only presentation for application-authored operation success and failure feedback, including validation, connection tests, queries, saves, and plugin actions. Do not repeat these messages in page bodies, panels, forms, or dialogs, and do not replace existing content or discard drafts with an outcome message. Keep necessary loading state, retry controls, and consent or destructive-action confirmations; a toast must still distinguish failure, conflict, and uncertain outcomes honestly.
- For destructive actions or required consent, identify the target and consequence briefly. Keep necessary warnings, choices, and recovery information; do not add repeated confirmations or long defensive disclaimers.
- Put optional explanation and technical diagnostics behind an appropriate help or details entry when needed. Keep accessible names meaningful. Do not shorten user content, code, raw tool output, or requested Agent answers merely to satisfy UI-copy brevity.
- Keep product copy in i18n resources with stable keys, and preserve meaning across locales. Review new and touched copy with each feature; do not restyle approved layouts or perform unrelated bulk rewrites to enforce this rule.

## Mandatory GPUI Kit reuse

- Preserve the approved Sailry information architecture and three-column layout. Compose that layout with GPUI Kit components; do not reproduce the old Flutter widget implementation or its separate styling system.
- Before implementing a control or interaction, inspect the selected GPUI Kit version's components, documentation, and examples. Use an existing supported component whenever it meets the requirement, including navigation, headers, buttons, inputs, tabs, lists, trees, tables, resizable panels, dialogs, menus, tooltips, and command/search overlays.
- Reuse the framework's semantic theme and sizing APIs for colors, typography, borders, radii, spacing, and hover/focus/disabled/selected states wherever available. Use one application theme configuration; do not create page-local palettes or a parallel token system. Product-specific column widths and layout constraints may use named application constants where the framework has no corresponding token.
- Keep framework focus, keyboard, accessibility, overlay, and theme-switching behavior intact. Do not substitute hand-painted lookalikes or static mock controls for available interactive components.
- Application-owned UI modules contain business composition and small necessary adapters, not one-to-one wrappers around every Kit component, copied framework internals, or another generic UI library.
- Custom rendering or controls require a verified framework gap. Explain the gap to the user and record the reason with the implementation before adding it. The planned Ghostty terminal renderer is such a boundary; ordinary controls around it still use Kit.
- Review component and theme reuse at each UI-stage acceptance, alongside relevant interaction tests. Visual similarity alone is not evidence that the framework components were reused.

## Functional parity and visual regression

- Deliver the whole product through the authorized tasks, not just the UI preview. Track UI implementation, real service integration, local verification, and remote verification separately; mark a task complete only with evidence for its stated scope.
- Use the committed old Code Flutter implementation as the reference for existing entry points, resource ownership, actions, and conversation behavior. Restore missing functions through Kit composition, not copied Flutter styling. Record the source revision and any verified framework gap with the affected feature.
- Preserve host -> project -> worktree -> session/terminal ownership even when the worktree layer is hidden in navigation. An architecture refactor does not authorize deleting existing functions.
- Treat user-approved visual components and layouts as the baseline. Connect functionality to those components without restyling them. Functional additions may require local layout adjustments; explain the reason and regression-test the affected baseline. Do not redesign unrelated surfaces.
- All right-click menus must use OS-native context menus; left-click menus must use Kit menus/popovers. Verify platform support rather than presenting a toolkit fallback as OS-native behavior.
- For affected UI workflows, run relevant automated behavior and interaction tests. Screenshot capture, pixel assertions, screenshot baselines, and screenshot review are not required; do not add or run screenshot tests unless the user explicitly requests them. Investigate reported layout issues without making screenshot validation a delivery gate.
- Interaction tests, OS-level E2E, performance measurements, and real-service E2E are different evidence; report exactly which ran and any coverage gaps.
- Extend end-to-end coverage as real services are connected, using isolated test resources for local and remote paths. UI fixtures and mocked services cannot establish whole-product acceptance. When automation cannot cover a platform behavior, perform and record manual acceptance instead of declaring it tested.

## Preservation and safety

- External reference worktrees are read-only unless the user authorizes otherwise.
- Preserve source provenance and license notices when importing reviewed code. Do not copy the entire old dirty tree or reset it to an older Git HEAD.
- Do not commit `.runtime`, `.preview`, credentials, `.npmrc`, local Cargo overrides, generated build directories, or private endpoint data.
- No destructive operations on old worktrees, user projects, sessions, credentials, or databases are part of the UI-preview phase.
