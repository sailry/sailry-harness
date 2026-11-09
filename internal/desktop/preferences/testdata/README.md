# Controller preference fixture

`preferences-rust.json` covers every field in the reviewed v1 controller Data
schema, including nullable values, a legacy unordered pin and a maximum u64 pin
order. `preferences_fixture.rs` deserializes and serializes it with the pinned
serde 1.0.229 / serde_json 1.0.151 format. Its declarations preserve the reviewed
controller field order and serde attributes; UUID values use their human-readable
string representation and paths use PathBuf.

`TestPrefsRoundTrip` also adds the optional Go-only `surfaces.blur` field. The
Rust Surfaces declaration accepts this unknown field and retains its ordinary
main/sidebar semantics. This extension is omitted until the user sets it.
