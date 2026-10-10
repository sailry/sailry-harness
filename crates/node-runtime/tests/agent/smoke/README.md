# Local conversation smoke tests

`cases.json` contains short, reusable prompts with isolated input files. The
opt-in test runs each case through local and paired Node transports and checks
file bytes, native chart data, and math delimiters. It prints one JSON metric per
case with the selected model, outcome, elapsed time, and reported token usage.
These are functional smoke checks, not SWE-bench or Terminal-Bench scores. Math
syntax checks do not prove desktop rendering; the desktop Markdown and streaming
interaction tests cover that boundary separately.

Run manually with an explicitly selected provider from an inactive
profile. Credentials are read only and used only against that provider's endpoint;
only that provider record is copied into temporary profiles for execution and history.
OAuth refreshes, if needed, affect only the temporary profile. This test is ignored in ordinary
Cargo runs and is not wired into CI. It incurs model usage.

```sh
SAILRY_ACCEPTANCE_PROFILE=/absolute/inactive/profile \
SAILRY_ACCEPTANCE_PROVIDER_ID=<provider-id> \
SAILRY_ACCEPTANCE_MODEL=<model-id> \
cargo test --locked -p sailry-node-runtime --features test-support \
  --test agent smoke::local_and_remote_conversations -- --exact --ignored --nocapture
```

Set `SAILRY_SMOKE_CASE=files`, `chart`, or `math` to run one case first. Each case
has a three-minute deadline; expiry stops the turn and fails the test. Short
prompts and answers reduce typical use but are not a guaranteed spending cap.
Use the same cases, model revision, reasoning effort, permissions, and fresh file
fixtures for manual comparisons. Do not infer another product's score or collect
its account history from Sailry's metrics.
