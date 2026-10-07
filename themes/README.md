# Desktop theme packages

Each directory under `themes/` is a portable v1 theme source. Import its directory
from Settings → Appearance → Import. Imported packages are stored under
`<profile>/desktop/themes/<id>/`. Repository directories are not automatically
installed or embedded in application builds.

A package contains `theme.json` and local raster resources under `assets/`.
The manifest requires `version: 1`, an ID matching its installed directory,
a name, author/license metadata, and both `light` and `dark` variants.
Colors and sizing in each variant's `theme` use the pinned GPUI Kit ThemeConfig.
Omitted values inherit Sailry's configured Kit defaults.

## Composer and buttons

Each variant may set `composer.border_width` (0–4 logical pixels). The color uses
Kit's `input.border` token; the surface and radius retain their Kit theme tokens.
The default width is zero.

The optional `composer.top_left` and `composer.bottom_right` assets draw fixed-size,
transparent corner ornaments. Their `height` is the square size (16–96 pixels).
Decorations paint behind content without taking input focus or installing actions.

`icon.send` and `icon.stop` replace the two distinct button glyphs.
`button.send` and `button.stop` optionally add image textures beneath them.
Button colors and gradients use the `button.primary.*` Kit tokens. Kit still owns
hover, pressed, disabled, keyboard activation and accessible names. Missing image
resources use the normal controls; the send image never substitutes for Stop.

## Welcome cards

Each variant's `cards` object accepts `explore`, `build`, `review` and `plan`.
Each card may configure solid `background`, `foreground`, `detail`, `icon`,
`hover`, `active` and `border` colors; `border_width` (0–4 pixels); and
`radius` (0–32 pixels). Omitted fields retain the existing Kit-based appearance.
The matching `icon.explore`, `icon.build`, `icon.review` and `icon.plan` assets
replace individual 24-pixel glyphs. Labels, prompt actions and responsive layout
remain application-owned.

## Raster decorations

PNG, JPEG, WebP and GIF resources are supported; SVG and remote URLs are rejected.
Use transparent PNG for art shared across light and dark modes. Each variant can
set its own `opacity` (0–1), `alignment` ([horizontal, vertical], each 0–1),
`fit` and `height`. `cover` fills and crops, `contain` preserves the whole image,
and `fill` stretches it. Background slots also accept `tile`, which repeats
without stretching at `tile_size` logical pixels wide (32–1024, default 256),
preserving the image's aspect ratio. Repeating textures must have matching edges.

The Great Sage package uses a large transparent landscape with `cover`, rather
than a repeating pattern, and reduces decoration opacity in light mode.
Compress production PNGs and size small ornaments/icons for their displayed size;
large wallpapers should retain enough detail for the window.

