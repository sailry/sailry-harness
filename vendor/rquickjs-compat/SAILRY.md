# Source and dependency identity

This is the unmodified `crates/shell/rquickjs-compat` facade from
`longbridge/gpui-kit` revision `0c830f4d257e69fdd17200650533ab4ca9a40cc0`
(Kit 0.7.0), with its exact manifest, re-exports and identity tests. The upstream
repository's Apache-2.0 notice is retained alongside its published package
license declaration.

Kit's LLRT bridge resolves this registry facade, which re-exports its JIT VM
types. Sailry Node keeps its existing rquickjs 0.12.2 implementation through
the official `DelSkayn/rquickjs` Git revision
`635f9322ccc875e3f9851e7f75ec538cf3274fd5`, the exact revision recorded by
that published 0.12.2 package. Separate Cargo source identities prevent Kit's
facade from replacing Node's execution VM. No Node VM upgrade or shared
business/runtime state is introduced.
