# Source and dependency identity

This is the unmodified `crates/shell/rquickjs-compat` facade from
`longbridge/gpui-kit` revision `1ccf011620da24d633f7cd6d3c11ca2e97798dcf`
(Kit 0.7.1), with its exact manifest, re-exports and identity tests. The upstream
repository's Apache-2.0 notice is retained alongside its published package
license declaration.

Kit's LLRT bridge resolves this registry facade, which re-exports the same
QuickJS JIT 0.12.11 types selected by Shell at revision
`96593cd7a54440ac060a391b8baea7e9a3cf1393`. Sailry Node keeps its existing
rquickjs 0.12.2 implementation through
the official `DelSkayn/rquickjs` Git revision
`635f9322ccc875e3f9851e7f75ec538cf3274fd5`, the exact revision recorded by
that published 0.12.2 package. Separate Cargo source identities prevent Kit's
facade from replacing Node's execution VM. No Node VM upgrade or shared
business/runtime state is introduced.
