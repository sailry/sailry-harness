# Source and local changes

Source: `longbridge/gpui-kit`, revision
`0c830f4d257e69fdd17200650533ab4ca9a40cc0`, the Kit, Shell, Assets,
Component Shell, WebView and FPS packages (0.7.0). Their Apache-2.0 license
and upstream notices are retained. Each manifest resolves workspace inheritance
in place; the product uses its root lockfile.

These integration packages retain that exact source and public API. Their
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
