# Source and local changes

Source: `longbridge/gpui-kit`, revision
`0c830f4d257e69fdd17200650533ab4ca9a40cc0`, the Kit, Shell, Assets,
Component Shell, WebView and FPS packages (0.7.0). Their Apache-2.0 license
and upstream notices are retained. Each manifest resolves workspace inheritance
in place; the product uses its root lockfile.

These integration packages retain that source and public API except for the
local changes described below. Their
GPUI snapshot dependencies are updated together to `=0.3.8`, following the
snapshot selection in upstream Kit revision
`87d10ae5e1299d1be18670a37570c5402f952e34`. This avoids selecting a second
GPUI snapshot through native-platform, asset or scripting dependencies without
importing the unrelated Kit 0.7.1 component changes. Base and Component use
the existing patched packages beside this directory; integration dependencies
use sibling packages. The unchanged Component macros dependency stays pinned
to the original upstream revision.

The separately vendored `rquickjs-compat` facade remains the product's shared
binding adapter. It is not duplicated inside Shell. Upstream tests are retained;
some reference examples, Story or other checkout fixtures outside these copied
packages and are not standalone product acceptance tests.

Native Assets enables RustEmbed's `debug-embed` feature. Debug application bundles
must retain their icon resources independently of the source checkout, matching
release bundles. Filesystem loading blocked the first native frame when macOS
could not access the external source volume before presenting a permission prompt.
The asset paths and public API are unchanged.

The WebView wrapper adds `new_hidden` for native views built with
`with_visible(false)`. At the pinned revision, `new` always stores
`visible = true`, and its only way to align that flag with a hidden Wry view
is `hide`, which unconditionally transfers keyboard focus to the parent.
Hidden construction shares the existing constructor and sets both native and
stored visibility without a focus call. `hide` transfers focus only from a
stored-visible view, so dropping an already-hidden background tab preserves
another view's responder. The existing `new` and visible-view hide behavior
remain unchanged. Sailry's native browser workload checks actual AppKit
responders or Windows focus handles during hidden operations.
