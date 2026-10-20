# llama.cpp and ggml

Sailry uses unmodified official llama.cpp **b11756** runtime assets from
https://github.com/ggml-org/llama.cpp/releases/tag/b11756.
The locked archive and ordinary-file member identities are in
`scripts/package/llama-runtime.lock.json`, copied into distribution notices.
llama.cpp and its bundled ggml sources use the retained MIT license in
[llama-LICENSE](llama-LICENSE).

The official macOS runtime requires macOS 13.3. The application retains its
macOS 13.0 minimum and loads this optional service only where supported.
The Linux x64 runtime requires glibc 2.34, libstdc++ with GLIBCXX_3.4.29 and
the system OpenMP library (`libgomp.so.1`). The official Linux ARM64 runtime
requires glibc 2.38 and is not distributed with Sailry's glibc 2.34 Host.
Windows x64 and ARM64 use the official CPU runtime packages, including their
OpenMP runtime. Its original Apache-2.0 with LLVM exceptions and MIT notices
are retained in [llvm-OpenMP-LICENSE](llvm-OpenMP-LICENSE). Model inference explicitly selects the CPU; mandatory linked
macOS backend libraries are retained without enabling GPU execution.
