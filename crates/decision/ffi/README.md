# Pinned native bindings

`generated/{llama,backend,base}.rs` were generated offline with rust-bindgen 0.72.1 from
llama.cpp `b11756`, commit `d80619397c2ba5cf1366c29f7d43699ca3a159b9`.
They are source bindings; builds do not run bindgen or compile C/C++.

The exact upstream inputs are:

- `include/llama.h`: SHA-256 `961430f89e2553dbcd99e3e6c59826af9ec143f05911520c74f804d986ff3cd2`
- `ggml/include/ggml.h`: SHA-256 `475beaa7e959be8c383279d9ce9d5191f37426e6b338270cfb12b26a798129d3`
- Other included headers come from the same commit, without edits.

Generation uses `Builder::header`, `clang_arg("-Iggml/include")`,
`layout_tests(true)`, `generate_comments(false)`, `derive_debug(false)`,
`derive_default(false)`, `dynamic_library_name` and
`dynamic_link_require_all(true)`. Allowlisting retains only the public functions
used by `engine.rs` and `runtime.rs`. Pointer-only model, context, vocabulary,
backend and callback tensor types are opaque. The checked by-value model/context
parameters and batch keep their generated field and layout assertions.
The generated library constructors pass `path.as_ref()` to libloading 0.9,
whose `AsFilename` bound replaces the older generic `AsRef<OsStr>` argument.
The public `llama_vocab_get_attr` enum preserves the pinned vocabulary's control
and whitespace attributes when trusted tokens are separated from literal input.

The handwritten laya/openjev preparation and calibration follow
`tools/server/server-decision.cpp` at that same commit (SHA-256
`77b25922ba6d685fc178784beac64ee3e84c5f9129c6c88b9d2e8f404e1b5283`).
Tensor block sizes follow `ggml/src/ggml-common.h` and `ggml/src/ggml.c`.
Upstream code is MIT licensed; its license is preserved in `LICENSE-llama.cpp`.

The loader validates the packaged b11756 manifest and each ordinary native file
against `scripts/package/llama-runtime.lock.json`, including source member hashes
and the actual signed bytes on macOS. It loads one explicit CPU backend. Linux
and Windows x86-64 score only the CPU plugins listed in that lock, selecting the
highest positive score supported by the current CPU. `ggml_backend_score` is the
exported `int(void)` plugin entry declared by `ggml/src/ggml-backend-impl.h`
(SHA-256 `4e861c8e23d3d7b0d99d8e0836e68564af58f8597d75d7bbb732857d7a632fa7`).
The same tag's CPU CMake target compiles that detector without ISA flags or LTO.
The public `ggml_backend_load` validates the selected plugin's API version before
registering it. Windows resolves DLL dependencies from the verified program
directory and System32. The loader never scans a directory, loads a system
runtime, or invokes private/staging decision APIs.
