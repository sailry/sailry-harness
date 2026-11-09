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

### Floating layers

Floating layers are built on MyGo's bases. Popovers are `kit.Popover`
(`internal/kit/popover.go`, PopoverBase with MyGo's popup motion, at
least as wide as the anchor), and dialogs are `kit.Dialog`
(`internal/kit/dialog.go`, adapted from Lobe MyGo's modal: DialogBase, a
tenth of the window down as Kit places them, the dimmed backdrop, a
close button, scale-and-fade motion, Escape and backdrop close). Tooltips
are `kit.Tooltip` (`internal/kit/tooltip.go`, adapted from Lobe MyGo's
`overlay/tooltip.go` and `overlay/placement.go`: TooltipBase, a Floating
tip on a `Placement` side that flips where there is no room, a spring
scale-in from the anchor, a content fade, an optional shortcut, and the
text as the anchor's description); every tip goes through it rather than
`Element.Tooltip`. Hover cards (`kit.SessionCard`, the usage and model
score cards) use TooltipBase with its 0.6 second delay and the tip's
`kit.TipMotion`. Left-click menus and selects are `kit.MenuButton`
and `kit.Select` (`internal/kit/menu.go`, on MenuBase, with the popup
motion, row color transition and turning chevron of Lobe MyGo's
`overlay/dropdown.go` and `field/select.go`): MyGo's drawn
menus paint the window's Background and highlight with the theme's
Accent (the primary), and its Theme has no menu slot, so the kit menu
paints the floating surface, and Kit's rows: 26 high, the highlight in accent.background /
accent.foreground (`Palette.Hover` / `HoverForeground`), the icon or
check slot, label, muted trailing text or action, right-side check
(`CheckRight`) and shortcut, Kit's 2 DIP separators. A select's trigger
has MyGo's TextInput look (its padding, the theme's Surface, the input
border), so selects and text inputs share a height and fill everywhere,
and its dropdown is as wide as the trigger (`MenuOptions.Match`).
Right-click menus stay the system's (`Element.ContextMenu`).
`kit.Markdown` (`internal/kit/markdown.go`) is adapted from Lobe MyGo's
`markdown.Markdown`: every Markdown view goes through it with MyGo's
`markdown.Options` passed whole; it fills only an unset `Theme` (the
`Document` or `Chat` variant's from the palette), `Reveal` (text as it
arrives: MyGo's paced reveal asks for display-rate frames, past
`kit.FPS`) and `CodeBlock` (`kit.CodeBlock`).
`kit.Alert` (`internal/kit/alert.go`) is adapted from Lobe MyGo's
`feedback.Alert` (MIT, notice kept): MyGo has no alert. `kit.Skeleton`,
`kit.SkeletonText` and `kit.SkeletonAvatar` (`internal/kit/skeleton.go`)
are adapted from Lobe MyGo's `display.Skeleton*`: Kit's skeleton color
(`Active`, half of it for `Secondary`), a pulse or sweep painted at
`kit.FPS`, still with Reduce Motion. `kit.Image`, `kit.ImageGroup` and
`kit.ImageViewer` (`internal/kit/image.go`, `imageviewer.go`,
`imagechrome.go`) are adapted from Lobe MyGo's `display.Image`,
`ImageGroup` and image viewer: every picture (thumbnails, attachments,
document pictures, the panel's preview, the composer's cards) is an
Image, a Skeleton while it loads; the viewer drives the caller's
`kit.Viewer` with the reference app's zoom limits, fit, pan bounds and
keys, its chrome Floating pills of IconButtons. `kit.Segmented`
(`internal/kit/segmented.go`) is adapted from Lobe MyGo's
`action.Segmented` on MyGo's SegmentedBase: every segmented choice and
in-page view switch (Installed | Available, details tabs, market kinds,
Document | Source) uses it; the track on `Muted`, the chosen segment
inverted (`Foreground` behind `Background`-colored text) with a sliding
indicator, as high as a text input. Document tab strips (the panel's
and the pages') stay tabs. `kit.ToastHost` (`internal/kit/toast.go`, from
Lobe MyGo's feedback.ToastHost) draws MyGo's toast queue once at the
window's root: stacked bottom right, spread on hover, the type's icon
and color, close and action. Floating panels paint through
`kit.Floating` (`internal/kit/floating.go`): a frost shader
(`internal/kit/frost.go`, a `ui/shader` backdrop blur composited over the
opaque popover color, then that color again, with a dither) and a
1 DIP edge, no shadow. `glass.Blur` keeps the backdrop's alpha, so over the
translucent window its sharp backdrop shows through. It is the one place
their surface is set: menus, selects, popovers, dialogs, toasts, tooltips, hover
cards, the reference picker, find bars, the selection toolbar and the
image viewer's bar.

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
