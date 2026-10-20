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

Run `python3 crates/decision/tests/golden.py` to prepare the pinned assets.
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
Cases cover English and Chinese, all three question types, literal input masks
and trained laya head trimming. Rust compares typed responses at a fixed `1e-4`
numeric tolerance and requires exact usage and option identities.

Use the existing runtime packagers to place the pinned libraries and manifests
beside the test executable, then run the ignored native tests. The two coexist
tests must each run in a fresh test process to cover both initialization orders.

```sh
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
