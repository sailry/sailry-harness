# Application icons

`sailry-windows.svg` keeps the existing ribbon geometry and colors from
`internal/desktop/exporter/assets/sailry-mark.svg`. Its centered square canvas
reduces the transparent padding so the ribbon fills 96% of the icon's height.
The shared `sailry.png` retains its original padding for Mobile.

Regenerate the 1024-pixel Windows PNG from the repository root with ImageMagick:

```sh
magick -background none assets/branding/sailry-windows.svg -strip PNG32:assets/branding/sailry-windows.png
```

MyGo's top-level `icon` supplies the Windows executable and installer icons at
16, 24, 32, 48, 64, 128 and 256 pixels. The pinned MyGo version has no Windows
icon override, so Linux and Web also use this fallback. macOS keeps its separate
`macos.icon` override, `Sailry.icns`; do not regenerate that approved asset from
the Windows source.
