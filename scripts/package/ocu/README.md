# Bundled computer-use helper

`ocu-helper.lock.json` pins Open Computer Use commit
`8fdda42639c94fa2ce22ea2d9d706b5919a83b42`, the source archive SHA-256 and the
reviewed patch SHA-256. `ocu-helper.py` verifies these inputs before applying the
patch and running the upstream native build script. The artifact cache includes
the locked inputs, target, compiler, development identity and Sailry icon.
Neither application startup nor a tool call downloads an executable.

macOS builders need Swift 6.2 or newer and the macOS SDK. Windows helpers use Go
1.22 or newer plus Node to read upstream version metadata. Go embeds
`runtime.ps1`; no npm runtime is shipped. Architecture validation checks the
produced Mach-O or PE file. Cross compilation does not verify Windows UI
Automation, installation or signing on a Windows desktop.

`bash scripts/run-desktop.sh` stages the helper automatically: inside the
development app on macOS and beside the execution binary on Windows.
`bash scripts/build-host.sh debug aarch64-apple-darwin` stages the macOS Host and
its development helper together in `target/host-artifacts/aarch64-apple-darwin`.
The macOS and Windows packaging scripts also stage their target's helper.

For an explicit development helper directory:

```sh
python3 scripts/package/ocu-helper.py --target aarch64-apple-darwin \
  --output target/helpers --cache target/ocu-helper-cache --development --sign -
SAILRY_HELPERS_DIR="$PWD/target/helpers" cargo run --locked -p sailry-host
```

Release packaging stages the unsigned helper before signing nested code and the
helper app, followed by the enclosing Desktop app. Host archives carry a
separately signed helper app. Desktop keeps `sailry-ocu.json` in
`Contents/Resources`, outside the nested-code directory; its inventory is updated
after helper signing and before outer application signing. Ad hoc signing is
available for local builds;
Developer ID signing and notarization use the existing packaging credentials and
verification flow. The helper keeps its own entitlements rather than inheriting
the Desktop app's permissions. The production bundle identifier is
`dev.sailry.computer`; development uses `dev.sailry.computer.dev`. Stable signing
identity and actual TCC continuity must be verified for a distributed upgrade.

## Scoped session patch

`scoped-sessions.patch` preserves the upstream native tools and annotations. It
adds connection ownership to occlusion and parked-window records, preserving the
first original state. Only the final owner restores it; restoration failures keep
the record for retry. Explicit restore releases the caller and reports a window
still in use. Diagnostic connections hold no tool state. Cursor ownership and
revision checks prevent an old connection or timer from hiding a newer cursor.

The stdio proxy sends a private `endSession` request at EOF and waits for its
acknowledgement. A negative or lost acknowledgement causes a nonzero proxy exit.
The app agent remains alive; session cleanup clears its connection's query and
snapshot indexes. Only process shutdown restores all owners. The global
turn-ended notification is not used by the embedded helper.
When a connection closes after failed restoration, the helper releases its owner
UUID and keeps the original capture for retry during later session cleanup.
New active owners retain that capture and prevent premature restoration.

The patch also aligns permission guidance, bundle metadata and the helper name
with Sailry, and makes MCP instructions respect existing authorization and
ordinary host approvals. Native tool schemas and platform-specific read-only
annotations remain upstream-owned.

The patch includes ordinary ownership tests, without permanent GUI interaction
tests. Python helper fixtures cover locked inputs, architecture, nested signing
and private cleanup acknowledgements when an explicitly built macOS helper is
selected through `SAILRY_OCU_HELPER`. Native macOS app interaction requires the
helper's Accessibility and Screen Recording grants. Tests and compilation do not
establish signing credentials, notarization or Windows device acceptance.

MIT and derived Cua Driver/yabai notices, plus libwebp's BSD license, are retained
in `third_party_licenses/open-computer-use-*` and copied into helper artifacts.
