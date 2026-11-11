# llama.cpp and ggml

Sailry uses unmodified official llama.cpp **b11756** runtime assets from
https://github.com/ggml-org/llama.cpp/releases/tag/b11756.
The locked archive and ordinary-file member identities are in
`internal/localmodels/decision/llama-runtime.lock.json`. llama.cpp and its
bundled ggml sources use the MIT license retained in
[`native/llama-LICENSE`](../native/llama-LICENSE).

The Windows runtime packages include the LLVM OpenMP runtime (`libomp.dll`).
Its original Apache-2.0 with LLVM exceptions and MIT notices are retained in
[llvm-OpenMP-LICENSE](llvm-OpenMP-LICENSE).

Minimum system and library requirements of each target are recorded in the lock
file. Decision inference reports `Unavailable` on a target that does not meet
them.
