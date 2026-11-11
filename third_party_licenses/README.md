# Third-party notices

Sailry's own code is licensed under [Apache-2.0](../LICENSE). Third-party
materials keep their own licenses. A notice lives beside the code or asset it
covers where one package owns it; this directory holds the notices that have no
single owning package, and this index lists every location.

## In this directory

| Component | License | Notice |
| --- | --- | --- |
| ADK-Rust sandbox policy, adapted in `internal/node/process` | Apache-2.0 | [adk-support.md](adk-support.md), [adk-LICENSE](adk-LICENSE) |
| OpenAI Codex patch format, adapted in `internal/node/files` | Apache-2.0 | [codex.md](codex.md), [codex-LICENSE](codex-LICENSE), [codex-NOTICE](codex-NOTICE) |
| Agent Plugins schema in `schemas/agent-plugins` | Apache-2.0 | [agent-plugins.md](agent-plugins.md), [agent-plugins-Apache-2.0.txt](agent-plugins-Apache-2.0.txt) |
| AI Logo brand marks, linked from `github.com/yldm-tech/ai-logo/mygo` and drawn through `internal/desktop/icons` | MIT, with per-brand terms | [ai-logo.md](ai-logo.md) |
| libghostty-vt terminal engine, shipped by the MyGo terminal plugin | MIT | [ghostty-terminal.md](ghostty-terminal.md), [ghostty-MIT.txt](ghostty-MIT.txt) |
| llama.cpp and ggml runtime | MIT | [llama.md](llama.md) |
| LLVM OpenMP runtime in the Windows llama.cpp packages | Apache-2.0 with LLVM exceptions, MIT | [llvm-OpenMP-LICENSE](llvm-OpenMP-LICENSE) |
| agent-desktop computer-use helper | Apache-2.0 | [agent-desktop.md](agent-desktop.md) |
| ModelDial public radar data, downloaded at runtime | CC BY 4.0 | [modeldial.md](modeldial.md) |
| Context7 and GitHub MCP integrations in `plugins/` | MIT | [context7.md](context7.md), [github-mcp-server.md](github-mcp-server.md) |
| Go modules linked into the binaries | Per module | [go-modules.md](go-modules.md) |
| Earlier Sailry Code and Sailry Platform sources | Apache-2.0, Sailry-owned | [sailry-code-terminal.md](sailry-code-terminal.md) |

## Beside the code and assets

| Component | Notice |
| --- | --- |
| Goose-derived agent operations, compaction and provider adaptations | `internal/node/agent/model/LICENSE.goose`, `internal/node/agent/output/LICENSE.goose` |
| Rig request layouts | `internal/node/agent/model/LICENSE.rig` |
| Pi tool preview policy | `internal/node/agent/output/LICENSE.pi`, `internal/node/agent/output/NOTICE` |
| oh-my-pi Claude wire behavior in model and provider code | `internal/node/agent/model/LICENSE.omp`, `internal/node/providers/LICENSE.omp` |
| GPUI Kit color names | `internal/desktop/theme/LICENSE.gpui-kit` |
| GPUI shell plugin prelude | `internal/desktop/plugins/js/LICENSE.gpui-shell`, `internal/desktop/plugins/js/SOURCE` |
| Reicon, Remix Icon, Lucide and Simple Icons platform logos | `internal/desktop/icons/svg/LICENSE-*` and `PLUGINS-SOURCE.md` in the same directory |
| Latin Modern Math font | `internal/desktop/conversation/LICENSE-LATIN-MODERN-MATH` |
| html2canvas, ECharts, MathJax and Mermaid export assets | `internal/desktop/exporter/assets/*-LICENSE`, `echarts-NOTICE` and `provenance.json` in the same directory |
| llama.cpp, ONNX Runtime and agent-desktop runtimes | `native/llama-LICENSE`, `native/onnxruntime-LICENSE`, `native/agent-desktop-LICENSE` and the lock files beside them |
| miniaudio capture header | `native/capture/SOURCE` |
| iroh transport and its Rust crates | `native/iroh/SOURCE`, `native/iroh/Cargo.lock` |
| Mobile icons, visuals and project monsters | `apps/mobile/assets/icons/`, `apps/mobile/assets/visuals/licenses/`, `apps/mobile/third_party_licenses/` |
| Pairing service | `services/pairing-relay/LICENSE`, `services/pairing-relay/NOTICE` |
| Official plugins and themes | Each package under `plugins/` and `themes/` |

Release packaging collects the license files of the linked Go modules and of the
Rust crates in the iroh library into the package; see
`internal/node/host/deployment/licenses.sh`.
