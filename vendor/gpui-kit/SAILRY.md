# Source and local changes

Source: `longbridge/gpui-kit`, revision
`1ccf011620da24d633f7cd6d3c11ca2e97798dcf`, the Kit, Shell, Assets,
Component Shell, WebView and FPS packages (0.7.1). Their Apache-2.0 licenses
and upstream notices are retained. Assets also retains the Lucide license.
Each manifest resolves upstream workspace inheritance in place; the application
uses its root Cargo.lock and the rebased sibling Base and Component packages.

Kit's `gpui-fast` feature selects the public engine and every enabled layer
together. All Fast core, platform and web dependency declarations are pinned to
`=0.1.3`; GPUI macros and the additive upstream build dependencies retain Kit's
exact `gpui-pre =0.3.8` family. Component macros come from this same Kit revision.
The current `gpui-webview` package always selects Fast, replacing `gpui-wry`.

## Patch decisions

| Previous local change | Decision |
| --- | --- |
| Resolved workspace manifests and sibling package paths | Retained packaging adaptation, refreshed from current upstream requirements |
| GPUI 0.3.7-to-0.3.8 snapshot override | Retired; current upstream already owns that selection and coherent optional Fast propagation |
| Asset `debug-embed` | Retained; debug application bundles must carry icons independently of the source checkout. Asset paths and public APIs do not change. Reusable upstream candidate |
| Shared `rquickjs-compat` facade | Retained in the application's existing vendor directory; the omitted Shell-local duplicate is unnecessary. Binding and VM versions remain selected by the upstream Shell |
| Initially hidden WebView construction | Retained `new_hidden` and rebased through the current constructor, synchronizing managed composition without taking another view's responder. Reusable upstream candidate |
| Visible-only focus transfer on hide | Retired local implementation; current upstream `set_visible` already checks stored visibility before transferring focus |

WebView's retained hidden-construction support also exposes `build_hidden` through
the same upstream platform setup. This ensures hidden native surfaces participate
in composition from their first attachment, including Linux's managed X11 parent.

`WebView::set_scale_factor` exposes the upstream element's existing display-scale
synchronization for clipped hosts. The current upstream WebViewElement lays out
the whole rectangular child and does not apply the application's content-mask
clipping or mount lifetime. The application retains a small Canvas host for that
verified gap, calling managed scale, bounds and visibility methods. Composition
registration, native surfaces and release remain in this package. These two
public construction/geometry extensions are reusable upstream candidates.

macOS native view composition and Linux X11 composition come from this exact
upstream source. Windows retains the upstream native child-view behavior.
Unsupported Linux Wayland behavior is unchanged. Platform source support does
not establish Sailry's platform acceptance.

Upstream tests are retained. Some integration tests reference Story or other
checkout fixtures outside these copied packages and are not standalone product
acceptance tests. Package refresh does not claim a build, native composition
acceptance or performance result.
