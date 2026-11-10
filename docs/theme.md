# Theme

B8 stores controller-local installed packages under `desktop/themes` in the
isolated profile. `theme.Profile` validates complete snapshots, compares the
installed digest before replacement/removal and publishes a staged directory.
The gallery merges those snapshots with the pinned embedded themes on the UI
thread. Local import opens the existing details/preview dialog before Install.
Export writes a ZIP through the same staged native-save destination used by
downloads. Uninstall confirms the named package and restores the default if it
was selected.

`theme.Market` follows the website catalog and immutable source commit, verifies
resource SHA256 descriptors and the Rust package digest, and retains the existing
installation on acquisition/validation failure. Its bounded profile cache uses
the Rust v1 saved-catalog envelope. Network failures may use retained or embedded
catalog data; invalid fresh feeds do not replace a valid cache. Package updates
require strictly greater SemVer precedence.
Windows sharing a profile share its publication mutex and change observation,
so each gallery reloads after a successful import, update or removal. Theme
catalog loading drives the existing Market skeleton until completion. Update
checks distinguish current packages, available updates and catalog failure
through the existing theme toast keys.

Sailry's one application theme: the built-in default's semantic tokens
overlaid by the selected theme package, the MyGo widget theme derived from
them, and the package's surfaces: artwork, mode cards, messages, the
wordmark and animated effects (the reference implementation `apps/desktop/src/theme.rs`,
`theme/*.rs`, `settings/appearance*.rs`, `conversation/welcome*`,
`themes/`).

## Package and files

`internal/desktop/theme`, with the drawing in `internal/desktop/kit` and the pages that use
them.

| File | Holds |
|---|---|
| `theme/theme.go` | `Palette`, `Of`, `Apply`, `Look`, `Widget`, `IsDark`, the MyGo theme built from a palette |
| `theme/package.go` | **The contract**: `Package` and `Variant` read from theme.json v1, `Parse`, `Packages`, `Find`, `Builtin` |
| `theme/neutral.go` | The built-in theme's colors by theme.json key, Kit's fallbacks, status and base colors, emblem tints |
| `theme/color.go` | `Color`, an optional package color |
| `themes/`, `bundled.go` | The pinned official theme submodule, embedded as `sailry.Themes`: `great-sage`, `ink-dragon`, `holographic-rhythm` (`theme.json`, `assets/`, `ARTWORK.md`, `README.md`), `LICENSE`, `NOTICE` |
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

### Floating layers

Which component draws each floating layer, and every other component
choice, is in [components.md](components.md). Their one surface is
`kit.Floating` (`internal/desktop/kit/floating.go`): the frost shader of
`internal/desktop/kit/frost.go` over the opaque `Popover` color, toned by it again,
with a 1 DIP edge and no shadow. Menus paint it rather than MyGo's menu
look, which uses `Background` and `Accent` and has no theme slot.

## Wiring a backend

B3 validates current v1 manifests, nested fields, known Kit token names, slots,
resource paths, dimensions and effect budgets against the reference contract.
Parse retains a bounded decoded image snapshot, so drawing never reopens a
changed source resource. The two variants share each unique image.

The live controller persists theme, mode, effects and Main/Sidebar/Blur in
`desktop/preferences.json`; UI fractions are converted to stored percentages.
Installed theme packages join the embedded set through the B8 market service.

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
| `Theme`, `Mode`, `Effects`, `Surfaces` | Embedded/installed packages and controller profile preferences |
| The welcome wordmark's light | — |

## Tests

`internal/desktop/theme/package_test.go`: every embedded package parses with both
variants and every asset decodes; malformed JSON, a missing variant and a
invalid color fail; the palette overlays neutral, falls back for links and the
switch thumb, and fades popovers. `TestThemeInvalidFixtures` covers the source invalid metadata, styles, resources
and effects with their rejection reasons. `TestEffectSimulation` checks seed
variation, bounds, loop continuity and hidden travel endpoints.
`internal/desktop/kit/effects_test.go`: effects
ask for frames, and stop with Reduce Motion, the switch and a hidden window.
`internal/desktop/shell/shell_test.go`: every package paints in both modes, each
differently, with the welcome page; a mode card fills the draft; Appearance
applies a package, shows its details and turns effects off.

## Deviations

| Deviation | Reason |
|---|---|
| Effects animate at 60 fps; the reference caps them at 20 and only while the window is active | The prototype's frame cap is 60 through MyGo's only frame-rate parameter; a hidden window stops them |
| Pictures paint with `ImageRegion` and a white tint; tile repeats regions | MyGo's Image has no tile fit, and `Element.Opacity` renders a layer |
| The pulse halo is a radial gradient | GPUI paints a blurred drop shadow; MyGo's `Shadow` paints only outside the box |
| The wordmark's base dots are painted, and a radial glow in `wordmark.glow` follows the beam | The reference reads `glow` but paints only the ink; MyGo's Icon would not light single dots |
| Petals are quadratic curves | Close to the reference's cubic petals at their few-DIP size |
| Import, export, uninstall and update actions are unavailable in live mode until B8 | Package installation belongs to the market service; gallery Apply and the native menu/details dialog already work |
| `message.assistant.*`, `banner.*`, `brand`, `button.send/stop` art and `icon.*` of the rail are read but not drawn | No shipped package uses them |
| Pinned conversations use `icon.group.recent` | The reference declares the group icons but draws none |
