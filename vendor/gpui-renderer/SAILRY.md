# Source and local changes

The four packages are the published `gpui-pre`, `gpui-pre-apple`,
`gpui-pre-wgpu` and `gpui-pre-windows` 0.3.8 releases. Their manifests identify
Zed revision `279fe070bb389b79652e52065b2f001edcc0b11b`. Upstream source and
Apache-2.0 notices are retained; the family license is included here.

The snapshot is selected consistently by the vendored GPUI Kit 0.7.0 integration
packages, Base and Component. Kit remains at revision
`0c830f4d257e69fdd17200650533ab4ca9a40cc0`; its manifests and required line-wrap
compatibility changes follow the GPUI dependency update in Kit revision
`87d10ae5e1299d1be18670a37570c5402f952e34` without importing unrelated Kit changes.

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

The 0.3.8 Apple adapter supplies typed layer setters, drawable geometry and
Metal buffer integer types upstream. The former local geometry workaround is
removed. The backdrop port retains its Objective-C dependency for the cached
Metal Performance Shaders blur kernel; the upstream layer configuration and
iOS support remain intact.

`wgpu/tests/material.rs` validates the actual storage, WebGL and subpixel
shader sources plus the backdrop record stride without requiring the upstream
font assets omitted from the published package. These are shader/layout checks,
not GPU pixel, driver or Windows runtime acceptance.
