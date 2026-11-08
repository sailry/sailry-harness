# Sailry engineering rules

Read [ARCHITECTURE.md](ARCHITECTURE.md) before changing code. Build, test, CI and
release procedures are in [CONTRIBUTING.md](CONTRIBUTING.md); package rules are in
[`plugins/AGENTS.md`](plugins/AGENTS.md) and [`themes/AGENTS.md`](themes/AGENTS.md).

## Scope

- Finish the authorized task against its acceptance criteria. Do unlisted work only
  as a demonstrated prerequisite or blocking defect; optional improvements,
  speculative hardening and later-phase work are not completion gates.
- Follow the current architecture and accepted UI. A refactor does not authorize
  deleting existing functions. Update docs only when requested or required by a
  behavior change; internal plans and reports are not source.

## Git and workspace

- Check `git status` first. Preserve uncommitted files you do not own, stage only
  your own, and commit locally unless a push or PR is requested. Keep commits
  reviewable. External reference worktrees are read-only unless authorized.
- All local Rust builds share the root `target` directory (other worktrees set
  `CARGO_TARGET_DIR`). Run one Cargo process at a time, never create per-task target
  directories, and keep SDK, `PKG_CONFIG_PATH`, target and Rust flags stable.
- Reuse fixed-purpose `.runtime` directories and worktree paths, without run IDs,
  timestamps or UUIDs. Clear an invalid cache only after stopping its owner and
  inspecting it; cleanup never touches plugin or theme repositories, source
  worktrees, user profiles or unrelated source.

## Validation

- Choose checks from the change and its dependency boundaries, following the
  [test conventions](CONTRIBUTING.md#test-conventions). Reuse passing results until
  relevant code, dependencies, configuration or environment change.
- Diagnose failures and verify fixes; never weaken assertions or report unverified
  behavior as complete. Run `just ci pre-push` once before a push or PR; prose-only
  changes need diff review, not product builds.

## Language and product copy

- Source, identifiers, comments, logs, diagnostics, developer errors and public
  docs are English. Other languages live only in i18n resources under stable keys,
  or in fixtures that test Unicode.
- Copy is minimal: short labels, direct actions ("Save", "Retry"), consistent terms,
  no filler, no terminal punctuation on short UI text, open prompts ("Describe your
  task").
- Operation success and failure appear only as toasts, never repeated in pages or
  dialogs and never replacing content or drafts. Toasts still distinguish failure,
  conflict and uncertain outcomes. Destructive and consent prompts name the target
  and consequence once.

## GPUI Kit

- Check the pinned Kit components first and use one whenever it fits, with Kit's
  re-exported GPUI, semantic theme and sizing APIs, and one application theme. No
  page-local palettes, one-to-one wrappers or copied framework internals.
- Custom rendering needs a verified framework gap, explained and recorded with the
  code (for example the Ghostty terminal renderer).
- Right-click menus are OS-native; left-click menus use Kit menus or popovers.
- Approved layouts are the baseline: connect functionality without restyling them.

## Code organization

- Organize by feature inside the owning crate; keep `main.rs`, `shell.rs` and
  `pages.rs` thin. Prefer composition and small interfaces over speculative base
  types, pass-through layers or traits without a concrete need. Keep names concise
  and let scopes supply context ([limits](CONTRIBUTING.md#test-conventions)). Add a
  platform adapter only for a verified gap.

## Dependencies and versions

- Pin external dependencies and commit lockfiles. The exact `rig-core` version lives
  in the root workspace and `Cargo.lock`. No other desktop stack, Agent engine or
  JavaScript runtime without an authorized architecture change.
- Sailry-owned contracts stay at `v1` and change in place until the first release
  ([details](ARCHITECTURE.md#contract-and-storage-versions)).
- New Sailry plugin packages start at `0.1.0`. Changed content bumps the SemVer,
  usually the patch. Updates need strictly newer precedence; same-version changes,
  build metadata and older versions never offer one. Preserve installed
  configuration, enable choices and removed records.

## Product boundaries

- Preview data is clearly identified and never triggers models, tools, pairing,
  terminals or user-data changes.
- Mobile is a Flutter controller with only Client, Link and a thin FFI adapter. A
  desktop-only Rust test is not Mobile acceptance; desktop tasks do not expand into
  Mobile work.
- Usage statistics cover only Sailry's own sessions. An enabled model plugin may
  show its provider's account status separately. Never collect third-party tool
  histories or analyze third-party billing.
- Plugins own their UI and workflows; core provides scoped services
  ([details](ARCHITECTURE.md#extensions-and-plugins)).

## Safety

- Keep safeguards proportional: authenticated access, path confinement, credential
  protection, revision conflicts and destructive-action confirmation. No redundant
  approvals or speculative permission systems. Saves keep drafts on failure and
  report real conflicts or uncertain outcomes.
- No destructive operations on user projects, sessions, credentials or databases.
- Never commit `.runtime`, `.preview`, credentials, `.npmrc`, local Cargo overrides,
  build output or private endpoint data.
