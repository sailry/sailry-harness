# Immutable skill resources

`internal/node/agent/operations` supplies resources from the exact package
references admitted with a turn. Bundled metadata includes packages with skills
and no native operations. Resources remain independent of installation inventory;
the controller uses exact installed and enabled package versions for its catalog.

`load_skill` accepts the exact `package:skill` identity and returns the complete
`SKILL.md`, its real execution-Node directory and Python readiness. The loader
is serialized because later Python preparation binds the most recently loaded
skill to the turn. `read_skill_resource` accepts a normalized confined relative
path and may run concurrently. Loading resources grants no command permission.

Documents and resources are bounded at 128 KiB and must be complete valid UTF-8.
The source skill catalog budget is 64 KiB. Relative paths, metadata scalar types,
skill names and localized labels follow the source contract. No partial binary
or oversized result is returned. Reads verify the immutable package contents and
reject changed resources rather than substituting current bytes.

Embedded files materialize under the existing private profile package location,
`plugins/packages/<digest>`. Rooted filesystem handles confine materialization
and reads; existing files are verified and never overwritten. This is access to
the captured snapshot and does not update installation records. B8 resolves
bundled and installed versions by admitted digest and settings revision.
Required Python preparation uses the shared Node manager, retains a turn lease
and selects the latest loaded ready skill for commands. Plain standalone skills
return `python: null` when the bundled interpreter is unavailable. There is no
system interpreter substitution; see [Python](python.md).

Package conversation modes use captured host-scope declarations and validated
string options. One selected mode is allowed, package modes cannot combine with
Flagship, and a read-only mode requires Plan. Mode instructions retain ordinary
captured tool authority. No JavaScript execution occurs in this resource path.
