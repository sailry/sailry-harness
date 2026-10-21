# Native decision fixtures

`fixtures/models.json` pins the public model source, revision, size, SHA-256 and
license. `golden.py` caches these files in the reusable
`.runtime/local-model-host/models` directory. Model weights are never committed.

Julia is a trained laya model. The SmolLM2 file is an explicitly **synthetic
compatibility fixture** for the openjev C path. Its self-authored `systemone`
template, type and calibration metadata are listed in
`fixtures/synthetic-openjev.json`, alongside its new file identity and every
unchanged weight tensor's SHA-256. No weights are converted or trained. The
fixture does not demonstrate trained decision quality and is not a catalog
recommendation. The original model files remain unchanged.

Run `bash scripts/python.sh crates/decision/tests/golden.py` to prepare the pinned assets.
`--generate` additionally runs the official b11756 server from its verified
prebuilt archive and replaces the committed golden JSON for review. The server
uses an empty offload-device list, four CPU threads, no GPU layers or operation
offload, no weight repacking, a 1024-token context and one slot. The golden data
retains answers, usage and the complete final prompt from the server's debug
slots endpoint. Its token IDs round-trip through the server's tokenizer; Rust
requires identical token IDs and head positions before comparing the outputs.
Each oracle question starts after the public slot-erase operation, matching
Engine's independent forward passes without prefix KV reuse between questions.
Answers and input usage are combined into the original typed request's response.
The official multi-question path retains a shared prefix between decoder tasks.
It produces different quantized CPU values for this synthetic fixture; its first
response is retained as `shared_prefix_reference`, outside the independent
forward expectations. The questions, templates and token IDs remain unchanged.
The server's machine-specific model path is removed.
Cases cover English and Chinese, all three question types and trained laya head
trimming. Rust compares typed responses at a fixed `1e-4` numeric tolerance and
requires exact usage and option identities. The stock server removed a literal
`<mask>` from the Chinese state before rendering. `fixtures/oracle-inputs.json`
records that exact raw state and the bytes the historical oracle actually used;
the original request and expected token IDs and probabilities remain intact.

Separate literal-input cases use both pinned vocabularies and real forward calls
to check state, question identity, instructions and option text. User-written
control spellings remain unchanged text, tokenized with `parse_special=false`.
Only trusted template controls split the ordinary text, preserving token merges
across interpolation boundaries and the vocabulary's whitespace attributes.
The separate BERT case loads the exact upstream vocabulary fixture in
`vocab_only` mode. It verifies zero-token whitespace spans and literal control
spellings without loading BERT weights or claiming BERT inference. Julia and
SmolLM2 keep their own whitespace tokenization unchanged.

These checks are manual opt-ins because they download real public model weights.
The **Native decision inference** Actions workflow runs them through
`workflow_dispatch` on macOS ARM64, Linux x64 or Linux ARM64 standard runners.
These execute the native architecture without QEMU. They are hosted virtual
machines and do not provide physical-machine qualification. The workflow is
separate from ordinary CI. Host release jobs execute
`scripts/check-host.py` and `scripts/check-host-install.py` against the real
packaged service and installer.

From the repository root, run the same entry point locally. It builds the test
executable, stages the locked native libraries, verifies the pinned GGUF files
and runs real inference and each initialization order in separate processes:

```sh
bash scripts/check-decision-native.sh
```

The optional workflow `measure` input, or the same local command with `--measure`,
runs the existing 150-case synthetic corpus and writes
`.runtime/local-model-host/qualification/native-report.json`. It records warm
p50/p95 latency and whole-process peak RSS without performance thresholds. Labels
are author-assigned, and `default_qualified` remains false. Run that command on
the intended physical Linux machine for physical performance evidence; hosted
runner results do not substitute for it.

Run each coexistence check in a fresh process to cover both initialization orders.
Do not use `--generate` during comparison: that option intentionally replaces the
reviewed official golden data.

```sh
cargo test --locked -p sailry-decision --lib runtime::tests::loads_scored_cpu_backend -- --ignored --exact
cargo test --locked -p sailry-decision --lib runtime::tests::caches_verified_package -- --ignored --exact
cargo test --locked -p sailry-decision --lib engine::tests::native:: -- --ignored --test-threads=1
cargo test --locked -p sailry-decision --lib engine::tests::coexist::onnx_first -- --ignored --exact
cargo test --locked -p sailry-decision --lib engine::tests::coexist::gguf_first -- --ignored --exact
```

They execute a real ORT session before and after Julia inference and model drop.
`fixtures/coexist.onnx` is a self-authored ONNX IR 9 / opset 13 MatMul graph:
`[x, y] × [[2, 0], [0, 3]]`. It has one float input and output shaped `[1, 2]`.

The macOS b11756 build eagerly initializes its linked Metal registry. Sailry
excludes offload devices and disables model and context offload; this does
not claim that the official process has no Metal initialization.

Linux requires the system OpenMP runtime (`libgomp` on RHEL or `libgomp1` on
Debian/Ubuntu). The locked x64 Decision runtime needs glibc 2.34+, and ARM64 needs
2.38+; this does not raise the Host or ORT minimum. Native package verification
and dependency failures are cached until process restart. This cache never stores
model download, selection or cancellation failures.

The separate `runtime::tests::reports_missing_dependency` opt-in is for an isolated
Linux fixture without `libgomp.so.1`, with the same packaged libraries beside the
executable. It must return `Unavailable` and retain that reason on repeated checks:

```sh
cargo test --locked -p sailry-decision --lib runtime::tests::reports_missing_dependency -- --ignored --exact
```
