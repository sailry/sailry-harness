# Desktop theme packages

Each directory under `themes/` is a portable v1 theme source. Import its directory
from Settings → Appearance → Import. Imported packages are stored under
`<profile>/desktop/themes/<id>/`. Repository directories are not automatically
installed or embedded in application builds.

A package contains `theme.json` and local raster resources under `assets/`.
The manifest requires `version: 1`, an ID matching its installed directory,
a name, author/license metadata, and both `light` and `dark` variants.
Optional `description` introduces the theme, `package_version` identifies its
release, and `license.name` summarizes its license without replacing `license.text`.
The numeric `version` remains the manifest format version, not a release version.
Provided descriptions must be nonblank and at most 4 KiB, with ordinary line breaks
and tabs allowed. Release versions and license names must be nonblank, single-line
strings of at most 64 and 160 UTF-8 bytes, respectively.
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

Explicit card backgrounds use their configured color and alpha directly;
hover and active colors may also include alpha (`#RRGGBBAA`). Omitted backgrounds
retain Kit's normal tint. Optional `background.card.explore`,
`background.card.build`, `background.card.review` and `background.card.plan`
assets add noninteractive textures, clipped to the card radius behind its content.

## Window backgrounds and sidebar headings

`background.window` draws once behind the entire Shell, including navigation,
the feature rail and title bar. Existing surface transparency settings control
how much of it shows through. `background.content` and `background.sidebar` are
separate optional foreground decorations; omit them for a continuous wallpaper.

Sidebar group headings have default Kit icons. The `icon.group.hosts`,
`icon.group.activity`, `icon.group.unassigned`, `icon.group.projects` and
`icon.group.recent` assets replace their 16-pixel glyphs without changing group
labels, disclosure controls or keyboard interaction.

## Raster decorations

PNG, JPEG, WebP and GIF resources are supported; SVG and remote URLs are rejected.
Use transparent PNG for art shared across light and dark modes. Each variant can
set its own `opacity` (0–1), `alignment` ([horizontal, vertical], each 0–1),
`fit` and `height`. `cover` fills and crops, `contain` preserves the whole image,
and `fill` stretches it. Background slots also accept `tile`, which repeats
without stretching at `tile_size` logical pixels wide (32–1024, default 256),
preserving the image's aspect ratio. Repeating textures must have matching edges.

The Great Sage package uses a transparent gold-and-vermilion landscape as a
window wallpaper with `cover`, and reduces decoration opacity in light mode.
Its translucent cards share a small repeating grain texture and themed icons.
Compress production PNGs and size small ornaments/icons for their displayed size;
large wallpapers should retain enough detail for the window.
