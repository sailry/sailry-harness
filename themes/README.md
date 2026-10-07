# Desktop theme packages

Each directory under `themes/` is a portable v1 theme source. Import a directory or ZIP
from Settings → Appearance → Import. Imported packages are stored under
`<profile>/desktop/themes/<id>/`. Repository directories are not automatically
installed or embedded in application builds.

Importing a package with an installed ID replaces that theme, regardless of its
release version. Selection and light/dark mode are preserved; an active theme
refreshes immediately. The complete verified snapshot replaces old resources.
Failed replacement restores the previous package when possible; an unsuccessful
restore is reported as uncertain and retains the previous files for recovery.

The theme card's native context menu offers Apply, Details, Homepage, Export and
Uninstall. Export creates a ZIP with `theme.json` at its root and the referenced
assets. ZIP import also accepts one containing folder. A theme's optional
`homepage` must be an HTTP or HTTPS URL; themes without one disable that menu item.

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

## Message surfaces and wordmark

`messages.user` and `messages.assistant` accept optional solid `background`,
`foreground` and `border` colors, `border_width` (0–4 pixels) and `radius`
(0–32 pixels). Omitted values preserve the current Kit message slots. The user
style covers sent and pending prompts, including inline editing; the assistant
style covers the message body, leaving status headers and action footers separate.
Markdown body text follows the configured foreground; links, code and status
colors retain their semantic roles. These refinements do not replace selection,
copy, reference links or editing behavior.

Optional `message.user.top_left`, `message.user.bottom_right`,
`message.assistant.top_left` and `message.assistant.bottom_right` assets add
transparent corner ornaments (16–96 pixels), clipped within the surface.

`wordmark.foreground` and `wordmark.glow` independently color the welcome page's
square particles and their illumination. Defaults follow Kit's foreground.

`button.new_session` overlays an eventless, full-width top-edge ornament on the
sidebar's existing New Conversation button. Its `height` is 4–96 pixels; a thin
transparent PNG preserves the label and Kit's fill and interaction states.

## Global control colors

The theme's `colors` already exposes Kit's global button treatments:
`button.background`, `button.foreground`, `button.hover.background` and
`button.active.background`, plus the same fields under `button.primary`,
`button.secondary`, `button.danger`, `button.info`, `button.success` and
`button.warning`. Background fields accept gradients. Ghost controls use the
semantic secondary foreground, accent hover and button active colors; disabled
states remain Kit-derived.

Tabs use `tab.background`, `tab.foreground`, `tab.active.background`,
`tab.active.foreground`, `tab_bar.background` and
`tab_bar.segmented.background`. The Great Sage package supplies both normal and
selected text colors, translucent bars and gold selection fills in both modes.
