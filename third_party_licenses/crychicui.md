# CrychicUI components

Desktop links `github.com/ELY-Open-Source/CrychicUI` at the pseudo-version pinned
in `go.mod` (`v0.0.0-20261110123803-1c55e6a884bb`, commit `1c55e6a884bb`),
unmodified, through the controls and adapters described in
[ARCHITECTURE.md](../ARCHITECTURE.md#presentation-boundaries).
It is a Go module dependency: no CrychicUI source is copied into this
repository.

CrychicUI is licensed under MIT, Copyright (c) 2026 CrychicUI contributors; the
notice is retained in [crychicui-MIT.txt](crychicui-MIT.txt).

Its own third-party notices are retained unchanged in
[crychicui-THIRD_PARTY_NOTICES.md](crychicui-THIRD_PARTY_NOTICES.md):

- **Lucide icons 1.58.0** (ISC, with the Feather-derived icons under MIT),
  embedded by the module's `icon` package and linked into Desktop.
- **Mermaid documentation examples** in `diagram/testdata`: test data of the
  module's `diagram` package, not linked or shipped.
- **`github.com/Broderick-Westrope/pele-go`**, which the `diagram` package
  imports and Desktop links for `diagram.Mermaid`: see
  [pele-go.md](pele-go.md).
- The module's Go dependencies. Desktop links them at the versions in Sailry's
  own `go.mod`, listed in [go-modules.md](go-modules.md).

Update the pin, this file and both retained notices together.
