# Sailry Code and Sailry Platform sources

Several packages adapt behavior from the authors' earlier, committed Sailry Code
revision `67ae9fa0` and Sailry Platform (`harbor-file-service`) revision
`727ce0a`. The adapting files name their source revision in a leading comment:

- `internal/node/process/environment_unix.go`: account login environment
  initialization from `rust/crates/sailry-terminal-host/src/shell.rs`
- `internal/core/files*.go` and `internal/core/trash.go`: confined file saving,
  walking, copying, directory paging and trash relocation
- `internal/databases/databases.go`: external database execution
- `internal/node/git/service.go`: Git behavior

These Sailry-owned adaptations are released under [Apache-2.0](../LICENSE) by
their copyright owner. The adapted sections carry no additional file-level
copyright notices, and no third-party license applies to them.
