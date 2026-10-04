# block compatibility patch

Source: crates.io `block` 0.1.6, by Steven Sheldon, licensed MIT as declared
in the retained upstream manifest. Upstream does not include a separate
license file in this release.

The test helper comes from `SSheldon/rust-block` revision
`642ea4a4a5853a21b55b05c34832a5f1bb1af61c`, under its retained MIT manifest.

Changes:

- Replace the empty `Class` enum with an inhabited opaque `repr(C)` struct.
  `_NSConcreteStackBlock` is used only by address, so the pointer ABI is unchanged.
  This fixes Rust's `uninhabited_static` future-incompatibility warning without
  suppressing the lint. See https://github.com/rust-lang/rust/issues/74840.
- Restore the upstream C interop test helper omitted from the registry archive,
  pin it, and use the workspace's existing `cc` version instead of obsolete `gcc`.
- Declare the original 2015 edition and spell the existing default C ABI explicitly
  to avoid deprecated implicit-ABI warnings when building the local patch.

Remove this patch when a compatible upstream release fixes the opaque static.
The six upstream tests cover native block invocation, arguments, and heap copies.
