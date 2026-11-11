# Protocol fixtures

These fixtures transcribe the JSON literals and serialized constructors in the
Rust protocol tests at revision `eaad1b7f18e448c8c4e9e9286e19560c8a351ae1`.
The tests consume only files in this checkout. They do not execute Rust or read
an external source tree.

Each directory names the concrete Go type selected by `TestFixtureRoundTrip`.
The test includes every `.json` file, decodes it into that type and compares its
serialized value with the fixture after normalizing object key order. JSON
numbers retain their original precision.

| Fixture directory | Rust source test |
| --- | --- |
| `browser_action` | `browser/tests.rs`, `screenshot` |
| `command` | `message/tests.rs`, `network`, `process`, `mcp`; `terminal/tests.rs`, `defaults_missing_arguments_without_changing_paths`; `team/tests.rs`, `preserves_stored_selection_commands` |
| `decision_answer`, `decision_request` | `decision/tests.rs`, `preserves_wire_and_validation` |
| `effort` | `conversation/reasoning/tests.rs`, `distinguishes_budget_and_default` |
| `event` | `message/tests.rs`, `history::preserves_activity_event_shapes` |
| `fault` | `message/tests.rs`, `faults::preserves_wire_and_validation` |
| `installation_resource` | `installation/tests.rs`, `identifies_language_package_versions` |
| `mcp_server` | `plugin/mcp.rs` Server Serde defaults and the `is_true` reuse omission predicate |
| `output` | `message/tests.rs`, `network`, `mcp`, `receipts`; `terminal/tests.rs`, `preserves_shell_arguments_on_the_wire` |
| `progress` | `conversation/progress/tests.rs`, `preserves_wire_and_validation` |
| `question_spec` | `conversation/question/tests.rs`, `defaults_text_options`, `defaults_other_answers_to_disabled` |
| `role_model` | `role/tests.rs`, `keeps_absent_features_following` |
| `team_settings` | `team/tests.rs`, `defaults_to_eight_children`, `requires_a_positive_budget` |
| `topic` | `message/tests.rs`, `subscriptions::preserves_wire_and_validation` |

The listed paths are relative to the Rust protocol source directory. Random UUID
constructors use deterministic, nonnil UUIDs here. Canonical serialized defaults
are explicit when the Rust input literal omits a field that serialization adds,
including browser screenshot options, question options and navigation locales.
The maximum team concurrency fixture preserves the `u64::MAX` case from the
Rust test and verifies that comparison does not round large integers.

`commands.txt` records all 362 Command wire kinds in Rust declaration order,
including the explicit `set_session_team` rename of `SetSessionExecution`.
The Go v1 inventory appends the documented `apply_git_hunk` extension for native
working-tree review and `synthesize_speech` for read-aloud. Their strict
command/result cases live in the grouped Go tests; the captured Rust inventory and JSON fixture identities stay intact.
Unknown and malformed input cases remain table entries in the Go tests instead
of round-trip fixture files.

The RestoreBackup fixtures also capture `message.rs`'s online-location omission
and explicit local location. These and the MCP server fixtures pin declared
Serde behavior alongside the literal cases extracted from Rust tests.
