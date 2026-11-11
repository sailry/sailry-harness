# pele-go Mermaid renderer

Desktop draws the Mermaid diagrams of a reply with CrychicUI's
`diagram.Mermaid` ([docs/crychic.md](../docs/crychic.md#diagrams)), which lays
them out with `github.com/Broderick-Westrope/pele-go`. Desktop links pele-go at
the version CrychicUI's `go.mod` requires and Sailry's `go.mod` pins (`v0.2.0`),
unmodified, through that package and through
`internal/desktop/kit/crychic/mermaid.go`, which renders a source once before
the library does. It is a Go module dependency: no pele-go source is copied
into this repository.

pele-go is a Go port of Pele (https://github.com/obsidianmd/pele) at Pele
1.10.0. It is licensed under MIT, Copyright (c) 2026 Brodie Westrope (pele-go,
the Go port) and Copyright (c) 2026 kepano (Pele, the upstream project); the
notice is retained in [pele-go-MIT.txt](pele-go-MIT.txt).

The packages Desktop links import only the Go standard library: no cgo and no
other module. `go.sum` also lists `github.com/dop251/goja`,
`github.com/dlclark/regexp2/v2` and `github.com/go-sourcemap/sourcemap`, which
only pele-go's own tests use to run Pele's JavaScript bundle; they are not in
`go.mod`, not linked and not shipped, and neither is that bundle.

Update this file and the retained notice with the pin.
