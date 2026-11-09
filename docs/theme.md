# Theme

Sailry's one application theme: the built-in default's semantic tokens
overlaid by the selected theme package, the MyGo widget theme derived from
them, and the package's surfaces: artwork, mode cards, messages, the
wordmark and animated effects (sailry-public `apps/desktop/src/theme.rs`,
`theme/*.rs`, `settings/appearance*.rs`, `conversation/welcome*`,
`themes/`).

## Package and files

`internal/theme`, with the drawing in `internal/kit` and the pages that use
them.

| File | Holds |
|---|---|
| `theme/theme.go` | `Palette`, `Of`, `Apply`, `Look`, `Widget`, `IsDark`, the MyGo theme built from a palette |
| `theme/package.go` | **The contract**: `Package` and `Variant` read from theme.json v1, `Parse`, `Packages`, `Find`, `Builtin` |
| `theme/neutral.go` | The built-in theme's colors by theme.json key, Kit's fallbacks, status and base colors, emblem tints |
| `theme/color.go` | `Color`, an optional package color |
| `theme/builtin/` | The official packages, embedded: `great-sage`, `ink-dragon`, `holographic-rhythm` (`theme.json`, `assets/`, `ARTWORK.md`, `README.md`), `LICENSE`, `NOTICE` |
| `kit/art.go` | `Picture`, `Corners`, `Backdrop`, `Banner`, `ArtIcon`, `NewSessionArt` |
| `kit/effects.go` | The particle layers and `SetEffects` |
| `conversation/welcome.go` | The new session's welcome page and wordmark |
| `settings/appearance.go` | Settings › Appearance |

## Entry point

```go
// First in the window's view: the package and mode for the frame.
theme.Apply(c, app.Theme, app.Mode)
kit.SetEffects(app.Effects)

// Everywhere after: the frame's palette (or a ui.Themed subtree's).
p := theme.Of(c)

settings.Page(c, s, data, settings.Look{Theme: &app.Theme, Mode: &app.Mode, Effects: &app.Effects})
conversation.Welcome(c, messages, composerView, func(prompt string) { draft.Draft = prompt })
```

## View model

| Type / field | Meaning |
|---|---|
| `Package{ID, Name, Description, Release, Homepage, License}` | A theme's manifest; `Release` is `package_version` |
| `Package.Light`, `Dark` | Its `Variant`s; nil for `Builtin`, which has no art |
| `Variant.Theme.Colors` | Kit colors by theme.json key; unset keys keep the neutral values |
| `Variant.Theme.Radius`, `RadiusLg` | Kit's base (6) and large (8) radius |
| `Variant.Composer` | `BorderWidth` in the input border color; `Send`, `Stop` colors |
| `Variant.Cards[explore/build/review/plan]` | A mode card's background, foreground, detail, icon, hover, active, border, radius |
| `Variant.Messages[user/assistant]` | The user bubble's colors, border and radius; the reply's foreground |
| `Variant.Wordmark` | The welcome wordmark's ink and glow |
| `Variant.Assets[slot]` | `Decoration{Path, Fit, Alignment, Opacity, Height, TileSize}`; `Bitmap()` decodes it once |
| `Variant.Effects[surface]` | `Effect{Kind, Color, Count, Opacity, Size, Duration, Seed}` layers |
| `Palette` | Every theme.json color as a token (`TitleBar`, `Sidebar*`, `Surface` for group_box, `Popover*`, `Link*`, `Primary*`, `ButtonPrimary*`, `Button*`, `ButtonSecondary*`, `Tab*`, `Switch*`, `List*`, `Table*`, …), the radii, Kit's status and base colors, its `Package` and `Variant` |

The colors map onto the palette as `neutral.rs` and `css.rs` do: the
neutral theme first, then the package's keys; links fall back to the
primary colors and the switch thumb to the background; popovers are 95%
opaque. Every package and mode is built once, at start-up.

### Where the tokens apply

| Tokens | Where |
|---|---|
| `TitleBar` (45% over the material) | The header band and the rail |
| `Background` (83.5% over the material or window art), `SidebarBorder` | The main panel and its outline |
| `Sidebar` (at the panel's alpha), `SidebarForeground`, `SidebarAccentForeground`, `SidebarItem` | Navigation, settings navigation, rows |
| `ButtonPrimary`, `ButtonPrimaryHi`, `ButtonPrimaryActive`, `OnPrimary` | Primary buttons, new session, send and stop |
| `ButtonSecondary*` | Outline buttons |
| `Hover`, `Active`, `Foreground` | Ghost and icon buttons, tabs, the theme cards |
| `Primary*` → MyGo `Accent*`, `Switch`, `Selection`, `Ring` → `Focus`, `Input` → `Border`, `ScrollbarThumbHover` | MyGo's switches, segmented controls, inputs, selects, sliders, focus rings, scroll bars |
| `Surface`, `SurfaceForeground`, `SurfaceTitle` | Settings groups, resource cards, empty lists |
| `Popover`, `PopoverForeground` | Dialogs, the model picker, interaction popovers |
| `Link`, `Primary` | Reply links, emphasis |
| `Radius`, `RadiusLg`, `Radius2xl()` | Controls; cards and popovers; the composer, tool cards and mode cards |

### Surfaces

| Slot | Where |
|---|---|
| `background.window` + effects | Under the whole window |
| `background.content` + effects | The main panel |
| `background.sidebar` + effects | The navigation |
| `conversation` effects | Under the transcript |
| `hero.new_session` + effects | The welcome page's banner |
| `composer.top_left`, `.bottom_right` | The composer's corners |
| `message.user.top_left`, `.bottom_right` | The user bubble's corners |
| `button.new_session` | Over the sidebar's new-session button, rounded |
| `icon.explore/build/review/plan`, `background.card.*` | The mode cards' icons and textures |
| `icon.group.recent/unassigned/projects` | The sidebar's pinned, chats and projects headers |

### Effects

Embers rise, dust and petals fall, twinkles pulse in place, pulse breathes a
halo, scan sweeps a line; positions come from a hash of the index and seed
(`motion.rs`), so they are the same every run. They repaint at 60 fps
(`kit.FPS`) through `Painter.AnimationFrameRate` without building the view,
and stop with Reduce Motion, the effects switch, or a hidden window (MyGo
holds frames of an occluded one).

## Wiring a backend

Theme packages are files; a backend adds installed ones beside the
embedded ones and persists the choice.

```go
pkg, err := theme.Parse(data, os.DirFS(dir)) // theme.json and its assets
if err != nil {
	c.Toast(i18n.T(c, "appearance_import_invalid"))
}

app.Theme, app.Mode, app.Effects = prefs.Theme, prefs.Mode, prefs.Effects
// After Settings › Appearance changes them:
prefs.Save(app.Theme, app.Mode, app.Effects)
```

`Packages` is built at start-up; installing at run time would add the
package and its palettes there (not in this prototype).

## UI state vs backend data

| UI state (`App`) | Backend data |
|---|---|
| `Theme`, `Mode`, `Effects` | Installed packages, the saved preferences |
| The welcome wordmark's light | — |

## Tests

`internal/theme/package_test.go`: every embedded package parses with both
variants and every asset decodes; malformed JSON, a missing variant and a
named color fail; the palette overlays neutral, falls back for links and the
switch thumb, and fades popovers. `internal/kit/effects_test.go`: effects
ask for frames, and stop with Reduce Motion, the switch and a hidden window.
`internal/shell/shell_test.go`: every package paints in both modes, each
differently, with the welcome page; a mode card fills the draft; Appearance
applies a package, shows its details and turns effects off.

## Deviations

| Deviation | Reason |
|---|---|
| Effects animate at 60 fps; the reference caps them at 20 and only while the window is active | The prototype's frame cap is 60 through MyGo's only frame-rate parameter; a hidden window stops them |
| Validation is parse errors and missing variants only; slots, ranges and effect budgets are not checked | The strict check (`check.rs`) is the logic half |
| Pictures paint with `ImageRegion` and a white tint; tile repeats regions | MyGo's Image has no tile fit, and `Element.Opacity` renders a layer |
| The pulse halo is a radial gradient | GPUI paints a blurred drop shadow; MyGo's `Shadow` paints only outside the box |
| The wordmark's base dots are painted, and a radial glow in `wordmark.glow` follows the beam | The reference reads `glow` but paints only the ink; MyGo's Icon would not light single dots |
| Petals are quadratic curves | Close to the reference's cubic petals at their few-DIP size |
| Details are a group under the gallery; there is no context menu, import, export, uninstall, homepage or updates | Only embedded packages; no market or files in this prototype |
| The window's opacity controls are not on the page | Not in this phase |
| `message.assistant.*`, `banner.*`, `brand`, `button.send/stop` art and `icon.*` of the rail are read but not drawn | No shipped package uses them |
| Pinned conversations use `icon.group.recent` | The reference declares the group icons but draws none |
