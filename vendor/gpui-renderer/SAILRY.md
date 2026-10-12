# Source and local changes

The five packages are the published `gpui-fast`, `gpui-fast-apple`,
`gpui-fast-macos`, `gpui-fast-wgpu` and `gpui-fast-windows` 0.1.3 releases from
[Longbridge's GPUI Fast](https://github.com/longbridge/gpui-fast).
Their source and Apache-2.0 licenses are retained. Archive SHA-256 checksums
were verified against the crates.io version records before the port:

| Package | SHA-256 |
| --- | --- |
| `gpui-fast` | `92dacd7ce83d53868a5cfa9ab8454a6ce0972bf6254d80bfb3cd22af142545bc` |
| `gpui-fast-apple` | `5f6e45468ae9800f22c176fa80811a711cfecda49177584ea320ee232c7f4596` |
| `gpui-fast-macos` | `5dcfeb8197d71dc5acbc250d02b3975ab2d0fe3c62bfab25f8315f15e7d98074` |
| `gpui-fast-wgpu` | `f2385b31d9cb8c31538d3c3262ab0f4a7e6a8c0fc5f2166af1142ca179d02fe1` |
| `gpui-fast-windows` | `184961e00dee1261c107790203c9d32f3eecea342f143cb96512c13ec1d6cac1` |

This family is selected by GPUI Kit at revision
`1ccf011620da24d633f7cd6d3c11ca2e97798dcf`, including the official
`gpui-webview` integration. It preserves Fast's retained layout and rendering,
scroll layers, native composition, headless rendering, device recovery,
input and accessibility. No second GPUI backend is selected.

Sailry's approved backdrop/material extension was reviewed against the exact
published `gpui-pre`, Apple, WGPU and Windows 0.3.8 archives before being ported
onto the Fast packages. Its provenance remains `dux-web/gpui` revision
`b9b7df79854ba971dfea77650825014cdfdb12c0`, relative to upstream ancestor
`fcce238b5f5b15cb7f25b152988fcc29aa40118e`; the reviewed Sailry source is
`ea1359b3`. The old renderer source is not used as the Fast baseline.

The extension adds backdrop records and interleaved scene batches, bounded
snapshot/blur/lens passes and matching Metal, WGSL and DirectX shadow shaders.
Backdrop resources track the current render target size, including headless
targets. Ordinary controls remain Kit components and the application retains
its approved material parameters through `GlassEffect`.

Fast's scene replay and native composition preserve backdrop operations.
A backdrop samples the already painted window scene, so content containing
one uses Fast's existing direct paint path instead of caching the material in
an independent scroll tile. Other scroll layers remain cached. Translation and
tile hashes include the material's geometry and optical parameters. WGPU's
Fast recorder includes backdrop instances in its shared upload and inserts the
snapshot/blur passes without discarding its bind-group, path and layer caches.

The Apple shader build reads the selected sibling core's shader types so the
new record layout stays consistent. Its additional Objective-C dependency
owns the cached Metal Performance Shaders Gaussian kernel. Scenes containing
backdrops use the existing material-aware Metal encoder: Fast's grouped path
encoder does not interleave framebuffer snapshots or backdrop passes. Ordinary
path scenes retain that grouped encoder. Fast's layer configuration and native
view composition remain intact.

A GPUI backdrop samples its GPUI render target. Native WebView pixels are
composed by the OS into a separate surface and are not part of that texture.
The material's tint and optical parameters are preserved, but this extension
does not claim to blur the webpage across the native composition boundary.
Windows keeps the application's fill fallback because DirectX does not
implement the backdrop pass.

`gpui/src/scene/material.rs` covers batch boundaries, replay, balanced native
composition ranges, empty masks, direct scroll painting, translation and
material invalidation. `wgpu/tests/material.rs` validates the actual storage,
WebGL and subpixel shaders plus the backdrop record stride. These are
scene and shader/layout checks, not GPU pixels, driver, native composition or
Windows runtime acceptance.

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
layer-local scene indices without subtracting window scene origins.
`gpui/src/fast/layers/lists.rs` tests partial-copy rebuilding, the text-cache
substreams and later complete copies with different window scene origins.

`ListState::with_uniform_item_height` refreshes unmeasured height estimates while
retaining measured geometry, focus handles and scroll anchors. The selected Diff
component uses this public operation after font and layout changes; keeping old
estimates made its offscreen scroll extent stale at the same viewport width.

The core build registers its existing `rust_analyzer` condition, and the Apple
and macOS packages register the legacy `objc` macro's `cargo-clippy` feature
condition without enabling it. Other unexpected conditions still warn. Cocoa
deprecation allowances are confined to the existing compatibility adapter
modules and helper items; the pinned Objective-C ABI and runtime behavior are
unchanged. The retained-subtree reuse accessor is compiled only for its core
unit tests, rather than every consumer enabling `test-support`.
