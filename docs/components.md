# Components

Which component to use where. The component library is MyGo's own
(`github.com/egoist/mygo/ui`, `ui/editor`, `ui/markdown`, `ui/shader`) plus
ours in `internal/desktop/kit`. Theme tokens and surfaces are in
[theme.md](theme.md).

## Choosing

1. A MyGo component, when it fits as it is.
2. A Lobe MyGo component (mygo.lobehub.com), copied as source into
   `internal/desktop/kit` with its MIT header and the upstream path and revision
   (`github.com/lobehub/lobe-mygo` at `5b3ecde`). Never add Lobe MyGo as a
   module dependency.
3. Our own, built on MyGo's headless bases (`ButtonBase`, `PopoverBase`,
   `MenuBase`, `DialogBase`, `TooltipBase`, `SegmentedBase`, `SliderBase`,
   `ToastBase`, `CollapsibleBase`, `TextInputBase`), with custom drawing only
   where MyGo has no such figure (the reason stays in the doc comment).

Every floating layer (dialog, popover, menu, select, toast, tooltip, hover
card) paints through `kit.Floating`. Nothing else sets a floating surface.

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
| `Loader` | Own (custom drawing) | The sidebar's running indicator, dot matrix at `FPS` | Loading content (`Skeleton`) | conversation, settings, sidebar |
| `Ring` | Own (custom drawing; pinned MyGo has no progress circle) | A share of a whole as a 15 DIP circle (`RingSize`), or its sweep while loading, in a tone: context usage | Progress of an operation in a row (`ui.Progress`) | composer, conversation, live |
| `Indicator`, `ThinkingOf` | Own (custom drawing) | The transcript's thinking indicator | Anything else | conversation |
| `Orb` | Own (custom drawing) | A breathing dot | — | unused |
| `Shimmer`, `Sweep` | Own (custom drawing) | Text of a running activity; `Sweep` shares one band across a row's texts | Static labels | conversation, host |
| `Identicon`, `Agent`, `AgentColor` | Own (custom drawing) | A subagent's mark and color | User avatars | conversation, composer, settings, shell |

### Choosing a value

| Component | Origin | Use | Not for | Used in |
|---|---|---|---|---|
| `Segmented` | Lobe `pkg/action/segmented.go` (SegmentedBase) | In-page view switches and a few side-by-side choices; the active segment is inverted. `SegmentedProps.Icons` puts an icon before each label and `Disabled` dims the whole track | Document tab strips, which stay tabs | panel, settings, sidebar |
| `MenuButton`, `Menu`, `Item`, `Submenu`, `OpenMenu` | Lobe `pkg/overlay/dropdown.go` motion (MenuBase) | Every left-click menu, ⋯ row menus, and pickers with a small, known set of choices (the composer's project, worktree, mode, permission); `Submenu` nests one (MenuBase's SubmenuItem), as the Git actions menu | Right-click menus (`Element.ContextMenu`, the system's); long searchable lists | composer, conversation, panel, settings, shell |
| `SplitButton`, `SplitProps` | Own (ButtonBase, MenuButton) | Kit's DropdownButton: a main action joined to a caret menu of the others (Git's Stage and Commit) | A single action (`Secondary`) | panel |
| `Select`, `SelectField`, `SelectProps`, `SelectOption`, `SelectTrigger` | Lobe `pkg/field/select.go` chevron (MenuBase) | A form field choosing one string; trigger looks like `ui.TextInput`, dropdown as wide. `SelectField` is the same control over options with a placeholder and disabled choices; `SelectTrigger` is its frame for a select with a popup of its own (the searchable header filter) | Toolbars and composer pickers (`MenuButton`) | settings, sidebar, plugins, live |
| `InputOTP` | Lobe `pkg/field/otp.go` | One-time codes | Other text | settings |
| `SortableList` | Lobe `pkg/layout/sortablelist.go` | Lists the user reorders, by mouse and keyboard | Static lists | composer |

### Settings and records

| Component | Origin | Use | Used in |
|---|---|---|---|
| `Page`, `Group`, `Card`, `Row`, `WideRow` | Own | A settings page, its groups, the card of rows, a setting (`WideRow` for selects, inputs and sliders). `Page` returns its scroll root; a plugin's `SettingsPage` is this page | settings, plugins |
| `SwitchRow`, `SegmentedRow` | Own | An on/off or few-labels setting | settings |
| `SelectRow`, `TextRow`, `SecretRow`, `NumberRow`, `SliderRow`, `ButtonRow` | Own | The other one-control settings; prefer them over a hand-built `Row` | defined, not yet used |
| `Entry`, `Record`, `Action`, `TextAction` | Own | A record in a group's card, its text and small ghost actions | settings, host, panel |
| `MediaRow`, `Tile`, `TileSize`, `Tint` | Own | A record with a 36 DIP emblem; `Tint` is the pair of theme tints a name hashes to | settings |
| `Grid`, `ResourceCard` | Own | Plugin, skill, market and team cards. The card tells its tile and actions whether it is hovered, so a package emblem animates with its card | settings |
| `Toolbar`, `List` | Own | A page's title, count, search and actions over its list | settings (`Toolbar`); `List` unused |
| `Field` | Own | A labeled control in a dialog | host, settings, sidebar |

### Overlays

| Component | Origin | Use | Not for | Used in |
|---|---|---|---|---|
| `Dialog`, `BareDialog` | Lobe `pkg/feedback/modal.go`, `dialog.go` (DialogBase) | Modal forms and confirmations; `BareDialog` draws its own close (the welcome guide) | Outcomes (toast) | composer, conversation, host, settings, shell, sidebar, transfers, onboarding |
| `Details` | Own (on `Dialog`) | A long error or report as selectable Markdown | Short failures (toast) | settings |
| `Popover` | Lobe `pkg/overlay/popover.go` (PopoverBase) | Non-modal panels under an anchor (questions, pickers with content) | Plain menus (`MenuButton`) | composer, conversation, shell, transfers |
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
| `ui.Progress`, `ui.Spinner` | Determinate and indeterminate progress in rows | Not for loading content (`kit.Skeleton`) | settings, host, transfers, composer |
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

### Visual details

- Menu rows are 30 high (the doc comment in `menu.go` still says 26).
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
- A button whose operation runs is disabled and busy with `ui.Spinner`
  (14) before its label, from the press until the outcome's toast
  (`kit.PrimaryBusy`, `kit.SecondaryBusy`, `kit.DangerBusy`,
  `kit.TextActionBusy`; `kit.BusyAction` holds an icon button's place with the
  spinner). An operation that already shows its own progress (a bar, a stage
  line, a row state) gets no second indicator: its button is only disabled.
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
  `Element.Loop`, which runs at the display's rate.
