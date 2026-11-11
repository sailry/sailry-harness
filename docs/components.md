# Components

Which component to use where. The component libraries are MyGo's own
(`github.com/egoist/mygo/ui`, `ui/editor`, `ui/markdown`, `ui/shader`), then
CrychicUI ([crychic.md](crychic.md)), then ours in `internal/desktop/kit`.
Theme tokens and surfaces are in [theme.md](theme.md).

## Choosing

1. A MyGo component, when it fits as it is.
2. A CrychicUI component verified in [crychic.md](crychic.md), under its
   rules. It may replace one of ours where no function is lost.
3. Kit, where neither fits or where ours does more:
   - a Lobe MyGo component (mygo.lobehub.com), copied as source into
     `internal/desktop/kit` with its MIT header and the upstream path and
     revision (`github.com/lobehub/lobe-mygo` at `5b3ecde`). Never add Lobe
     MyGo as a module dependency;
   - our own, built on MyGo's headless bases (`ButtonBase`, `PopoverBase`,
     `MenuBase`, `DialogBase`, `TooltipBase`, `SegmentedBase`, `SliderBase`,
     `ToastBase`, `CollapsibleBase`, `TextInputBase`), with custom drawing
     only where MyGo has no such figure (the reason stays in the doc comment).

Every floating layer (dialog, popover, menu, select, toast, tooltip, hover
card) paints through `kit.Floating`. Nothing else sets a floating surface: a
CrychicUI component is content inside one, and one that draws its own is not
used ([crychic.md](crychic.md#floating-surfaces)).

## Kit components

Origin: **Lobe** is a copy of the Lobe MyGo file named; **Own** is ours on
MyGo bases.

### Buttons and status

| Component | Origin | Use | Not for | Used in |
|---|---|---|---|---|
| `Ghost`, `GhostLook` | Own (ButtonBase) | Transparent text buttons; `GhostLook` styles a bare base (a `MenuButton` trigger, Deny) | The main action of a dialog (`Primary`) | composer, conversation, panel, settings, shell, sidebar, host, transfers |
| `IconButton`, `IconButtonHint`, `IconLook` | Own (ButtonBase) | Every icon-only button; the label is its tooltip and accessible name, the hint a shortcut | Icons that are not buttons (`ui.Icon`) | everywhere |
| `Primary` | Own | The one main action of a dialog, card or form | More than one per group | composer, host, panel, settings, sidebar |
| `Secondary`, `SecondaryLook` | Own | Other actions beside a `Primary`; Cancel. `SecondaryLook` styles a bare base whose content the caller builds (a plugin's menu button and model trigger) | Toolbars (`IconButton`) | everywhere but onboarding |
| `Danger` | Own | Actions that remove something, after a confirmation | Deny on an approval (ghost) | conversation, settings |
| `Tag`, `Badge` | Own | A short label; `Badge` in a `Tone` | Counts in a toolbar (`Toolbar` count) | composer, settings, shell |
| `StatusDot`, `StatusRing` | Own | A state as an 8 DIP dot; `StatusRing` on a mark or avatar | State with words; prefer an icon in rows | settings, sidebar, shell |
| `Loader`, `LoaderIn` | MyGo `plugins/loading` (`loading.Swirl`) | The app's one loader, in the muted color or the color given: running work (the sidebar's running rows 12, the composer's running capsules and rows 14) and content being read (a centred page 20) | Loading content that has a shape (`Skeleton`); an operation a control started (`Busy`) | composer, conversation, live, panel, plugins, settings, sidebar |
| `Busy`, `BusyIn` | MyGo `plugins/loading` (`loading.Ring`) | The mark of an operation a control started, 14 (`BusySize`): before a busy button's label, in an icon's place, beside the value being saved. `BusyIn` takes the label's color on a filled button | Running work and loading pages (`Loader`); progress with a known share (`ui.Progress`, `Ring`) | composer, live, panel, plugins, settings, shell, sidebar |
| `Ring` | Own (custom drawing; pinned MyGo has no progress circle) | A share of a whole as a 15 DIP circle (`RingSize`), or its sweep while loading, in a tone: context usage | Progress of an operation in a row (`ui.Progress`) | composer, conversation, live |
| `Indicator`, `ThinkingOf` | MyGo `plugins/loading` | What a running turn does, 16 beside its phase: `loading.Pulse` thinking or waiting (held still for no known phase), `loading.LinearDots` replying, `loading.Swirl` while tools or subagents run | Anything else | conversation, plugins |
| `Shimmer`, `Sweep` | Own (custom drawing) | Text of a running activity; `Sweep` shares one band across a row's texts | Static labels | conversation, host |
| `Identicon`, `Agent`, `AgentColor` | Own (custom drawing) | A subagent's mark and color | User avatars | conversation, composer, settings, shell |

### Choosing a value

| Component | Origin | Use | Not for | Used in |
|---|---|---|---|---|
| `Segmented` | Lobe `pkg/action/segmented.go` (SegmentedBase) | In-page view switches and a few side-by-side choices; the active segment is inverted. `SegmentedProps.Icons` puts an icon before each label and `Disabled` dims the whole track | Document tab strips, which stay tabs | panel, settings, sidebar |
| `MenuButton`, `Menu`, `Item`, `Submenu`, `OpenMenu` | Lobe `pkg/overlay/dropdown.go` motion (MenuBase) | Every left-click menu, ⋯ row menus, and pickers with a small, known set of choices (the composer's project, worktree, mode, permission); `Submenu` nests one (MenuBase's SubmenuItem), as the Git actions menu. An item with a `Description` is a choice row (`ChoiceRow`, below), with `Item.Tint` and `Item.Warning` | Right-click menus (`Element.ContextMenu`, the system's); long searchable lists | composer, conversation, panel, settings, shell |
| `SplitButton`, `SplitProps` | Own (ButtonBase, MenuButton) | Kit's DropdownButton: a main action joined to a caret menu of the others (Git's Stage and Commit) | A single action (`Secondary`) | panel |
| `Select`, `SelectField`, `SelectProps`, `SelectOption`, `SelectTrigger` | Lobe `pkg/field/select.go` chevron (MenuBase) | A form field choosing one string; trigger looks like `ui.TextInput`, dropdown as wide. `SelectField` is the same control over options with a placeholder and disabled choices; `SelectTrigger` is its frame for a select with a popup of its own (the searchable header filter) | Toolbars and composer pickers (`MenuButton`) | settings, sidebar, plugins, live |
| `InputOTP` | Lobe `pkg/field/otp.go` | One-time codes | Other text | settings |
| `SortableList` | Lobe `pkg/layout/sortablelist.go` | Lists the user reorders, by mouse and keyboard, never with up and down buttons. `Divided` rows sit in a settings `Card` with a divider between them and lift as a `Floating` panel; `Disabled` dims the handles while the order is fixed | Static lists | composer, plugins |
| `AppearancePicker`, `ProjectGlyph`, `ProjectColor`, `ProjectColors` | Own (`Popover`, `Ghost`) | A project's icon and color: the glyph button with its popover of swatches and icons, and the glyph itself wherever a project shows | Other icons or colors | sidebar |
| `SortableList` | Lobe `pkg/layout/sortablelist.go` | Lists the user reorders, by mouse and keyboard | Static lists | composer |

### Settings and records

| Component | Origin | Use | Used in |
|---|---|---|---|
| `Page`, `Group`, `Card`, `Row`, `WideRow` | Own | A settings page, its groups, the card of rows, a setting (`WideRow` for selects, inputs and sliders). `Page` returns its scroll root; a plugin's `SettingsPage` is this page | settings, plugins |
| `LeadRow`, `RowText` | Own | A `Row` with a leading `RowIcon` slot (a brand mark or icon; an empty slot keeps the text aligned with its neighbours); the title and description of any row at the row metrics | plugins |
| `SwitchRow`, `SegmentedRow` | Own | An on/off or few-labels setting | settings |
| `SelectRow`, `TextRow`, `SecretRow`, `NumberRow`, `SliderRow`, `ButtonRow` | Own | The other one-control settings; prefer them over a hand-built `Row` | defined, not yet used |
| `Entry`, `Record`, `Action`, `TextAction` | Own | A record in a group's card, its text and small ghost actions | settings, host, panel |
| `MediaRow`, `Tile`, `TileSize`, `Tint` | Own | A record with a 36 DIP emblem; `Tint` is the pair of theme tints a name hashes to, which a package icon's tile also uses | settings |
| `Grid`, `ResourceCard` | Own | Plugin, skill, market and team cards | settings |
| `Toolbar`, `List` | Own | A page's title, count, search and actions over its list | settings (`Toolbar`); `List` unused |
| `Field` | Own | A labeled control in a dialog | host, settings, sidebar |

### Overlays

| Component | Origin | Use | Not for | Used in |
|---|---|---|---|---|
| `Dialog`, `BareDialog` | Lobe `pkg/feedback/modal.go`, `dialog.go` (DialogBase) | Modal forms and confirmations; `BareDialog` draws its own close (the welcome guide) | Outcomes (toast) | composer, conversation, host, settings, shell, sidebar, transfers, onboarding |
| `Details` | Own (on `Dialog`) | A long error or report as selectable Markdown | Short failures (toast) | settings |
| `Popover` | Lobe `pkg/overlay/popover.go` (PopoverBase) | Non-modal panels under an anchor (questions, pickers with content) | Plain menus (`MenuButton`) | composer, conversation, shell, transfers |
| `Snapped` | Own (`Popover`) | A popover page that needs its room in the frame it shows: the panel takes its place and size at once and leaves as before | Pages the panel can ease between | composer |
| `Tooltip`, `TooltipProps`, `Placement`, `TipMotion`, `Attach` | Lobe `pkg/overlay/tooltip.go`, `placement.go` (TooltipBase) | Every tooltip; never `Element.Tooltip` | Hover cards with content | composer, conversation, host, panel, settings, shell, transfers |
| `SessionCard` | Own (TooltipBase) | A session's hover card | — | shell, sidebar |
| `Anchored`, `TipsHidden`, `TipLayer`, `PopoverLayer`, `CardLayer` | Own | Keying a hand-built overlay by anchor and layer; hiding its tip while a popover or menu is open | — | composer, conversation |
| `Floating` | Own (`frost.go` shader) | The surface of every floating panel, including find bars, the selection toolbar and the image viewer's bar | Cards on the page | composer, conversation, panel; all kit overlays |

### Feedback

| Component | Origin | Use | Not for | Used in |
|---|---|---|---|---|
| `ToastHost` | Lobe `pkg/feedback/toast.go` (ToastBase) | Once, last in the window's view, over every dialog | — | shell |
| `Toast`, `Outcome` | Own | An operation's outcome: done, failed, conflict, uncertain | Text in pages or dialogs | composer, host, settings, shell, sidebar |
| `Error`, `Notify`, `Retry` | Own | An error toast with detail; a neutral toast; an error toast with Retry | Inline errors | host, shell |
| `Result` | Lobe `pkg/feedback/result.go` | Empty and unavailable states, load failures, centered | Operation outcomes | composer, conversation, host, panel, settings, shell, transfers |
| `Alert` | Lobe `pkg/feedback/alert.go` | An inline persistent notice in a tone | Operation outcomes | settings |
| `Skeleton`, `SkeletonText`, `SkeletonAvatar` | Lobe `pkg/display/skeleton.go` | Content that is loading | Running work (`Loader`, `Shimmer`) | host, settings |

### Content

| Component | Origin | Use | Used in |
|---|---|---|---|
| `Markdown`, `MarkdownProps`, `MarkdownTheme` | Lobe `pkg/markdown/markdown.go` (`ui/markdown`) | Every Markdown view; `Chat` or `Document` variant | conversation, panel |
| `Code`, `CodeOptions`, `CodeBlock`, `CodeTheme` | Own (`ui/editor`) | Code and editable text with highlighting; `CodeBlock` is Markdown's fence hook | conversation, panel |
| `Diff`, `DiffOptions` | Own (`ui/editor` diff) | A change, unified | conversation, panel |
| `Image`, `ImageGroup` | Lobe `pkg/display/image.go` | Every picture: thumbnails, attachments, previews | composer, conversation, panel |
| `ImageViewer`, `Viewer` | Lobe `pkg/display/imageviewer.go`, `imagechrome.go` | The full-window picture viewer | composer, conversation, panel |
| `KeyText`, `KeyName` | Own | Shortcuts as menus show them | panel, settings, shell |

### Theme art and motion

| Component | Origin | Use | Used in |
|---|---|---|---|
| `Picture`, `Corners`, `Backdrop`, `Draws`, `Banner`, `Brand`, `ArtIcon`, `HasArtIcon`, `NewSessionArt` | Own | A theme package's art on its slots ([theme.md](theme.md)) | composer, conversation, settings, shell, sidebar |
| `SetEffects` | Own (custom drawing) | The theme's particle layers on or off | shell |
| `FPS`, `Fast`, `EaseOut`, `EaseOutCubic` | Own; `EaseOut` from Lobe `motion` | Frame cap (60) for looping drawings; motion timing | composer, conversation, settings, transfers |

## MyGo built-ins used directly

| Component | Use | Notes | Used in |
|---|---|---|---|
| `ui.Switch` | On/off controls | Through `kit.SwitchRow` on settings pages | settings |
| `ui.Checkbox`, `ui.Radio`, `ui.RadioGroup` | Multiple and single choices | Unchecked boxes fill with the theme's `Background`; see caveats | composer (questions), settings (plugins, market) |
| `ui.TextInput`, `ui.TextArea` | Text fields | `kit.Select` matches their height and fill | settings, host, sidebar (editor), panel |
| `ui.TextInputBase`, `ui.TextAreaBase` | Inputs drawn in their own look (the composer, its queue and files, sidebar menus, search) | — | composer, sidebar, shell |
| `ui.Slider` | Numbers on a range | — | settings (terminal, appearance) |
| `ui.NumberInput` | Numbers typed | Only through `kit.NumberRow` (not yet used) | — |
| `ui.SliderBase` | A custom-drawn slider (the model effort track, `composer/track.go`) | — | composer |
| `ui.Progress` | Determinate progress in rows, and the indeterminate bar | Not for loading content (`kit.Skeleton`). `ui.Spinner` is not used: it is the classic mark of spokes, 17.5 DIPs unless sized, and turns under Reduce Motion; `kit.Busy` and `kit.Loader` stand for it | settings, host, transfers, composer |
| `ui.Scroll`, `ScrollHorizontal`, `ScrollBoth` | Scrolling regions | — | everywhere |
| `ui.Table`, `ui.List`, `ui.Divider`, `ui.Link` | Tabular file lists, the transcript's virtual list, separators, links | — | composer, conversation |
| `ui.CollapsibleBase` | Collapsing sections | — | sidebar, settings |
| `ui.Icon` | Monochrome glyphs from `internal/desktop/icons`, in the text color | Not for brand marks | everywhere |
| `ui.Image` | Brand marks in their own colors (`icons.BrandColor`, from the AI Logo module; `kit.Logo` and a menu item's `Image`) | — | settings, plugins |
| `ui.Themed` | A subtree with a changed MyGo theme | — | composer |

## Conventions

### Choosing between controls

- In-page view switches are `kit.Segmented` with the active segment
  inverted. Document tab strips (the panel's and the pages') stay tabs.
- A `kit.Segmented` inside a dialog, and the plugin details tabs, fill
  their width (`SegmentedProps{Block: true}`, the segments sharing it
  evenly); elsewhere it is as wide as its labels.
- Every searchable list of choices (the search, the Git branch and other
  pickers) is a `kit.Command`: the query over a divider, 36 DIP rows under
  group headings, a divider between groups. No page builds its own.
- Pickers with a small, known set of choices (the composer's project,
  worktree, mode and permission) are a `kit.MenuButton` dropdown, not a
  searchable command dialog.
- A row action that duplicates a menu item goes only in the row's ⋯ menu,
  not as an inline button too. Example: Connect on an unconnected
  connector.

### Specific screens

- **Approvals** (`composer/interactions.go`): a title line; the request in
  a light card (`p.Foreground.Alpha(0.05)`); then Deny as a plain ghost
  button and Allow, with a `MenuButton` dropdown of wider grants when there
  are any. No "deny with reason".
- **Question choices**: unchecked checkboxes and radios take the panel
  surface: wrap them in a `ui.Themed` scope whose `Background` is
  `ui.Transparent`, not the window's darkest color.
- **Tool rows** (`conversation/tools.go`): only a call that failed to run
  (`Call.Failed`) is red. An error the tool returned is a plain row with its
  error line. Denied, interrupted and not-executed rows use `p.Warning`.
- **Question history rows**: state by icon only, no state text.

### Floating layers and overlays

- Floating panels: frost blur `FloatingBlur` 18, tint `FloatingTint` 0.4,
  dark-mode lift `FloatingLift` 0.1, a 1 DIP edge, no shadow. `glass.Blur`
  is unusable: it keeps the backdrop's alpha, so in our translucent window
  the sharp pixels under the panel show through.
- Under the desktop's Reduce Transparency preference every frost surface
  (`Floating`, `Frost`, `Pane`) paints its opaque base instead of the backdrop
  blur, as MyGo asks of materials. Headless shell tests use that preference,
  which keeps the software shader interpreter out of each frame.
- A modal dialog goes through Kit (`Dialog`, `BareDialog`, `AlertDialog`,
  `ImageViewer`), which notes it on the window; one built on `Element.Modal`
  directly calls `kit.NoteModal(c)` while it shows. The window holds the
  native menu's commands behind a noted dialog (`kit.TakeModal`), which
  MyGo's own modal does not do.
- `kit.MenuButton` keeps a menu's state by its id on the window: an id is
  unique among everything shown at once (two panes of one view each need
  their own), and `kit.SweepMenus` drops the states no frame asked for.
- `kit.SortableList` cancels a lift when the list changes under it.
- Key a hand-built overlay panel with `kit.Anchored(panel, anchor, layer)`
  before building its children. Tooltips and hover cards are suppressed
  while a popover or menu is open (`kit.TipsHidden`); kit's own tooltips do
  this already.

### Row metrics

Every list row with a title and a description shares one vertical rhythm,
defined once in `kit/settings.go` and used by `Row`, `WideRow`, `LeadRow`,
`Entry`, `Record` and by a package's `SettingsRow`, `CardRow` and
`CardSummary`:

| Metric | Constant | Value |
|---|---|---|
| Title and description size | `RowFont` | 14 |
| Title weight | — | regular (`Record` titles are medium, `MediaRow` titles 16 medium beside their 36 tile) |
| Title to description | `TitleGap` | 5 |
| Padding above and below | `RowPad` | 12 |
| Minimum row height | `RowMinHeight` | 56 |
| Leading icon slot, gap after it | `RowIcon`, `RowIconGap` | 24, 12 (icon 20) |

A row's overflow menu is an icon button (`IconLook` with the ellipsis, 28),
named by its label for the tooltip and assistive technology, never an icon
with the word beside it. A plugin's `Menu` with an icon is drawn the same way.
A code block's Copy is one too (24, `content_copy`), in place of MyGo's
`CopyButton` with its text, and confirms with the `content_copied` toast.

### Text beside icons

A row centres its boxes, and a text's box is its line: the ascent and the
descent, whose middle is below the capitals' (0.36 DIP for the system font
at 14), and MyGo then snaps the baseline to a device pixel (down with Core
Text). Unlevelled, a 14 DIP label stands 0.86 DIP below its icon's middle at
scale 1 and 0.40 at scale 2, an ideograph 1.04 and 0.56, a 13 DIP mono
detail on the same line 1.19 and 0.76.

`kit.Level(c, text, size)` puts a single line of text level with the icons
of its row: it moves the box by the difference between where the line's
metrics put the baseline and where the capitals' middle needs it (half the
capital height of 0.7 em below the row's middle, to a whole DIP so no
platform's snapping moves it), with a margin above and its opposite below,
which keeps the box's size. `kit.LevelTop(c, text, size, height)` does the
same for the first line of a text that may wrap in a row aligned to its top,
whose icons take `(height - icon) / 2` above them. Levelled, the three are
within 0.3 DIP of the icon at both scales (`kit` `TestSweepSharesRow/level
with the icon`, and the transcript's own rows in `shell`
`TestShellRules/LoadingMarks`).

Levelled today: the transcript's tool, question, subagent, step and phase
rows, the composer's capsules, context bar buttons and settings rows, the
sidebar's group, project and session rows, `kit.Menu`'s rows and a code
block's language. A smaller text after a levelled label (an age, a
shortcut) stays unlevelled: its line already shares the label's baseline.
A new row with an icon and a text levels its text the same way; no row
offsets a text by hand.

### Visual details

- Menu rows are 30 high.
- A menu item with a description is a choice row, `kit.ChoiceRow` with a
  `kit.Choice`: the icon 16 in a tile 28 across, radius 8, filled with the
  row's tint at 14% and drawn in it; the label 13 medium over the description
  12 muted, 1 apart; 6 by 8 inside, the tile 10 before the texts, at most 400
  wide, where the description wraps. The chosen row has no fill: its check,
  14, follows the label in the tint, on either side the menu's checks are,
  and its tile is filled at 30%. The fill (`accent.background`) is only the
  highlight's, the pointer's or the keys'. The tint is one of `theme.Tints`
  (the lighter stop in dark mode, the darker in light); a row without one has
  the gray tile and its check in the text color, and a row without an icon
  has no tile. `Item.Warning` draws the description in the warning color.
  The rows are the composer's mode and permission menus, a plugin's choice
  menu with descriptions and the team editor's Add role menu (no icons: the
  two lines alone); the composer's settings popover draws its mode and
  permission pages with `ChoiceRow` on its own buttons, so they match.
- Toolbar and message action icons are 15 inside a 28 button
  (`kit.IconButton(…, 28, …)`; the `IconButton` comment still says 16).
- Colors that must be opaque use `kit.Opaque(c)`: `ui.Color.Alpha`
  multiplies the existing alpha, so `Alpha(1)` keeps a translucent token
  translucent.
- Empty and unavailable states of a page, a list or a document use
  `kit.Result`, including the ones a plugin declares (`EmptyState`, an empty
  `CardList` or `DataTable`). A picker, a menu, a popover or a dialog's body
  with nothing to list says so in one muted line of its own row height, as
  `kit.Command` does.
- Brand marks keep their own colors via `ui.Image`, never `ui.Icon`, which
  tints. Every brand mark comes from the AI Logo module through
  `internal/desktop/icons`: `icons.BrandColor(id)` for `ui.Image`,
  `icons.Brand(id)` (one color) where the surface tints its glyph, as a
  `kit.Tile` and the model picker's rail. `icons.BrandSlug` is the one mapping
  from the preset catalog's identifiers to the collection's slugs. `kit.Logo`
  is a mark at a size with the label's initial for a brand the collection
  lacks; `Item.Image` and `SelectOption.Image` put one before a menu item or
  a `SelectField` option.

### Feedback

- Success and failure appear only as toasts (`kit.ToastHost` once,
  `kit.Toast`, `kit.Error`, `kit.Retry`, `kit.Notify`), never repeated in
  pages or dialogs and never replacing content or drafts.
- A button whose operation runs is disabled and busy with `kit.Busy`
  (14) before its label, from the press until the outcome's toast
  (`kit.PrimaryBusy`, `kit.SecondaryBusy`, `kit.DangerBusy`,
  `kit.TextActionBusy`; `kit.BusyAction` holds an icon button's place with the
  mark). An operation that already shows its own progress (a bar, a stage
  line, a row state) gets no second indicator: its button is only disabled.
- Loading marks are MyGo's `plugins/loading` through Kit, one for each
  thing that goes on, none drawn by a page: `kit.Loader` (Swirl) for running
  work and content being read, `kit.Busy` (Ring) for the operation a control
  started, `kit.Indicator` (Pulse, LinearDots, Swirl) for a running turn's
  phase. A mark inside a control or beside 14 DIP text is at most 14
  (`kit.BusySize`), the size of the icons there; a turn's phase mark is 16,
  as the transcript's row icons; a centred page's loader is 20. Their color
  is a theme token given by Kit (muted, or the row's text color through
  `LoaderIn` and `BusyIn`); a subagent keeps its identicon in its own color.
  A text that names running work still shimmers (`kit.Shimmer`, `kit.Sweep`).
- OS notifications go through MyGo native, not a toast or a kit component.

## Theming caveats

- MyGo's drawn menus paint the theme's `Background` and highlight in
  `Accent`, and its `Theme` has no menu slot, so menus are `kit.Menu`, which
  paints `Floating` and highlights in `Palette.Hover`.
- MyGo fills an unchecked checkbox or radio with `Background`, the window's
  darkest color; on a floating or card surface scope it with `ui.Themed`
  (above).
- MyGo's `Accent*` is the palette's `Primary*`: switches, sliders, focus
  rings and selections follow the primary color, not the hover color.
- Popover tokens are 95% opaque; `kit.Floating` makes its base opaque with
  `kit.Opaque` before tinting.
- Looping drawings use `Painter.AnimationFrameRate(kit.FPS)`, not
  `Element.Loop`, which runs at the display's rate. The loading marks are
  the exception: MyGo's `plugins/loading` asks for `Painter.AnimationFrame`
  and has no rate option, so they repaint at the display's rate (60 frames a
  second on a 60 Hz display, 120 on ProMotion) while one shows. Only the
  marks paint again, nothing is rebuilt (20 loaders for a second: 59 frames,
  0 builds, `kit` `TestEffects/LoadingMarks`); a mark out of view or not
  built asks for no frame, and with Reduce Motion each holds a still pose
  and asks for none.

## Deviations

| Deviation | Reason |
|---|---|
| The loader's dot matrix, the thinking figures (Orbits, Globe, Ribbon, Ring) and the unused `Orb` are removed; `kit.Loader` and `kit.Indicator` show MyGo's `plugins/loading` marks | The user's decision: one set of ready-made marks. The loader is one color (it had the info, warning and danger colors by row) |
| `ui.Spinner` is replaced by `kit.Busy` and, on centred loading pages, `kit.Loader` | It is the classic mark of spokes the set leaves out, 17.5 DIPs unless sized (the composer's capsules showed it so), and it ignores Reduce Motion |
| The loading marks move at the display's rate, past the 60 fps cap | `plugins/loading` has no rate option; capping it would copy its drawing code |
| `kit.Level` assumes capitals 0.7 em high | MyGo's `FontMetrics` has the ascent, descent and line gap, no cap height; the system UI fonts are within 0.02 em of it |
