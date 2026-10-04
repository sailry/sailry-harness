# Source and local changes

The four packages are the published `gpui-pre`, `gpui-pre-apple`,
`gpui-pre-wgpu` and `gpui-pre-windows` 0.3.7 releases selected by GPUI Kit
0.7.0 (`longbridge/gpui-kit` revision
`0c830f4d257e69fdd17200650533ab4ca9a40cc0`). Their manifests identify Zed
revision `1a28cff4b409169bac058bca40dfbfeb7621d19b`. Upstream source and
Apache-2.0 notices are retained; the family license is included here.

Sailry's previously approved backdrop/material extension is ported from
`dux-web/gpui` revision `b9b7df79854ba971dfea77650825014cdfdb12c0`, relative
to its upstream ancestor `fcce238b5f5b15cb7f25b152988fcc29aa40118e`.
The port retains Kit's new scene, renderer
core, headless rendering, device recovery and input/accessibility behavior;
it does not downgrade or select a second GPUI revision.

The extension adds backdrop records and interleaved scene batches, bounded
snapshot/blur/lens passes and matching Metal, WGSL and DirectX shaders.
Backdrop resources track the current render target size, including headless
targets. Frames without backdrop records retain the normal renderer path.
Application materials use these APIs; ordinary controls remain Kit components.

The Apple adapter uses `objc2-foundation` geometry and `objc2` messaging for
drawable sizes instead of deprecated Cocoa types. Buffer ranges use Metal's
own integer type. The layer size and buffer-range semantics are unchanged.

`wgpu/tests/material.rs` validates the actual storage, WebGL and subpixel
shader sources plus the backdrop record stride without requiring the upstream
font assets omitted from the published package. These are shader/layout checks,
not GPU pixel, driver or Windows runtime acceptance.
