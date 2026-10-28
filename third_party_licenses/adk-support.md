# ADK-Rust support code

The following Node support modules are adapted from the reviewed ADK-Rust source
at commit `675c02b6c0df6d632b953a06a5a12d1f814b7780` in
<https://github.com/dux-web/adk-rust>, based on
<https://github.com/zavora-ai/adk-rust>:

- `crates/node-runtime/src/external_browser/webdriver/`: the used Chromium session
  methods and JavaScript string escaping from `adk-browser/src/session.rs` and
  `escape.rs`. Sailry retains explicit owner-controlled startup, its existing
  Chromium options and service faults; unused browser types and toolsets are
  omitted.
- `crates/node-runtime/src/external_browser/tests/schemas.json`: the 46 parameter
  schemas from `adk-browser/src/tools/`, with descriptions omitted, retained as
  test contracts for the unchanged browser package.
- `crates/node-runtime/src/process/sandbox/enforcement/`: the used policy,
  Seatbelt, bubblewrap, nested-path layering and seccomp implementation and
  ordinary tests from `adk-sandbox/src/sandbox/`. Sailry uses local service faults
  and its existing process owner; unrelated execution backends are omitted.
- `crates/node-runtime/tests/agent/evaluation.rs`: argument and Unicode word
  matching from `adk-eval/src/schema.rs` and `scoring.rs`, reduced to the existing
  integration assertions without scoring or evaluator infrastructure.

Copyright 2026 Zavora Technologies Ltd. These imported portions remain under
Apache-2.0. The retained upstream license notice is in [adk-LICENSE](adk-LICENSE).
Sailry's changes adapt module ownership, visibility, names and error boundaries;
the original copyright and source provenance remain alongside the imported code.
