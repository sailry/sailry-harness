# Source and local changes

The five packages are the published `gpui-fast`, `gpui-fast-apple`,
`gpui-fast-macos`, `gpui-fast-wgpu` and `gpui-fast-windows` 0.1.6 releases from
[Longbridge's GPUI Fast](https://github.com/longbridge/gpui-fast).
Their source and Apache-2.0 licenses are retained. Archive SHA-256 checksums
were verified against the crates.io version records before the port:

| Package | SHA-256 |
| --- | --- |
| `gpui-fast` | `41e1053e2088b5c249aa78b50642e61323ed80afebbec1e5b317871eec340c7f` |
| `gpui-fast-apple` | `0ca3837c127ec7791cce36beebb7c2c096690edac1a1a4dfe25df4bc0de910db` |
| `gpui-fast-macos` | `25c4d51e9cd5ad910cde91f9cf6986f9c71660bb7dbe24cccb989a55a1fe826a` |
| `gpui-fast-wgpu` | `9dcb49862ad77f0649f6693a761ef0ba0a9de944e1854b0bc4608aeffcf24855` |
| `gpui-fast-windows` | `911e1ac8562bfdcebbb1da361276247e3f966ca8b3bcb6c8fbd127803f88a637` |
| `gpui-fast-platform` | `67f2f069a3ba873d6e80913a77409b8d74c5e347ef86b5a5f53f0806d9e30489` |
| `gpui-fast-web` | `76c74e7736b7a5c4dce005407f393ce696d060814ae88f3bb9adea52953416da` |
| `gpui-fast-macros` | `e1d80ba862ddcfa3c2bea405c28c4f4d59f9853993456f871888cff5fb6f0c51` |
| `gpui-fast-linux` | `69290eae78fc156bfaeea297a4c12955fae84be224692e997800f6c4a996b121` |

This family is selected by GPUI Kit at revision
`1ccf011620da24d633f7cd6d3c11ca2e97798dcf`, including the official
`gpui-webview` integration. It preserves Fast's retained layout and rendering,
scroll layers, native composition, headless rendering, device recovery,
input and accessibility. No second GPUI backend is selected.

The entire Fast family remains pinned to `=0.1.6` by the consuming manifests
and the root Cargo.lock. The obsolete standalone macOS lockfile is retired;
standalone unit-test mirrors use private development locks, not product locks.

The within-window backdrop/material extension is retired by product decision.
Its scene records, tile exceptions, framebuffer copies, Gaussian and lens
passes, shader additions, extra Objective-C dependency and extension-specific
tests were removed against checksum-verified 0.1.3 release sources before this
upgrade and are not carried into 0.1.6. The WGPU and Windows implementation
source now matches the 0.1.6 releases. Standard Kit surfaces
own their theme fills, borders, shadows and animations. Fast's native WebView
composition and the application's OS window backdrop remain in use.

The retired extension's provenance is `dux-web/gpui` revision
`b9b7df79854ba971dfea77650825014cdfdb12c0`, relative to upstream ancestor
`fcce238b5f5b15cb7f25b152988fcc29aa40118e`; the reviewed Sailry source was
`ea1359b3`. It had been ported from the published `gpui-pre` 0.3.8 family.
The old renderer source is not a baseline for the remaining Fast patches.

`Window::is_hitbox_occluded` retains the current paint frame's clipped,
mouse-blocking area overlap query. Fast's point hover testing is not an area
query. The application needs this only for native child-view snapshot fallback
on platforms or attachments without native composition support.

Fast's test-support selectors follow the precise paint ranges that recorded
them. Reusing one subtree no longer restores all selectors from the previous
frame, which kept removed controls discoverable after a notified sibling was
rebuilt. Layer paint-range relocation checks selector-range containment
alongside input, cursor, tab and text metadata; an unrelated layer must not
be shifted into a reused subtree, which could underflow its selector index.
These corrections preserve rendering and the existing precise range replay.
`gpui/src/fast/tests/retained.rs` covers removed selectors beside a retained
sibling and layer selector ranges before, inside and after a reused range.

The AppKit composition adapter uses `NSView.setSubviews` when its native surface
order changes. AppKit preserves views present in both sibling arrays; removing
and re-adding every existing surface would detach a focused page when a hidden
sibling is created. Existing unmanaged siblings retain their order, while genuine
parent changes use AppKit's normal attachment lifecycle. Geometry and visibility
handling remain with the selected Fast and Kit owners. This does not restore
keyboard focus or alter Wry's application activation behavior.

A retained paint copy follows a virtual-list layer only when every held row
is inside the copied window record ranges, including all four text-cache
streams. A partial copy invalidates the layer's range stamps so its next use
repaints the rows before carrying their records. Relocation preserves
layer-local scene indices without subtracting window scene origins. The
scene-index reset is now supplied unchanged by upstream 0.1.6; only the missing
containment checks and partial-copy invalidation remain local.
`gpui/src/fast/layers/lists.rs` tests partial-copy rebuilding, the text-cache
substreams and later complete copies with different window scene origins.

`ListState::with_uniform_item_height` refreshes unmeasured height estimates while
retaining measured geometry, focus handles and scroll anchors. The selected Diff
component uses this public operation after font and layout changes; keeping old
estimates made its offscreen scroll extent stale at the same viewport width.

The core build registers its existing `rust_analyzer` condition, and the macOS
package registers the legacy `objc` macro's `cargo-clippy` feature condition
without enabling it. Apple no longer needs that registration after removing
the material extension's legacy Objective-C dependency. The macOS manifest
keeps its targeted warning-level registration instead of upstream's blanket
allowance; the other packages retain upstream's lint declarations. Cocoa
deprecation allowances are confined to the existing compatibility adapter
modules and helper items; the pinned Objective-C ABI and runtime behavior are
unchanged. The retained-subtree reuse accessor is compiled only for its core
unit tests, rather than every consumer enabling `test-support`.

The native Metal drawable-size fixture remains beside the unmodified layer
configuration and texture cleanup. It does not restore any material pass or
establish application composition acceptance.
