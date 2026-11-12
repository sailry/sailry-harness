# CrychicUI

`github.com/ELY-Open-Source/CrychicUI` (MIT) is a MyGo component library and
Desktop's second UI library, after MyGo's own controls. Desktop links it at
the pseudo-version pinned in `go.mod` (`v0.0.0-20261110123803-1c55e6a884bb`)
against Sailry's own MyGo pin; notices are in
[third_party_licenses/crychicui.md](../third_party_licenses/crychicui.md).

## Rules

- **Order of preference.** A MyGo control; then a CrychicUI component verified
  below; then Kit or custom drawing, where neither fits or where ours does
  more ([components.md](components.md#choosing)).
- **Replacing one of ours.** A CrychicUI component may replace a Kit or
  feature component where it is better suited and no function is lost. List
  what ours does, show the replacement keeps each item, and move every caller
  in the same change. Where ours does more, ours stays: see
  [What ours keeps](#what-ours-keeps).
- **Floating surfaces stay Kit's frost.** Menus, popovers, dialogs, hover
  cards and sheets paint through `kit.Floating` (`kit.Dialog`, `kit.Popover`,
  `kit.MenuButton`, `kit.Tooltip`, the hover card). CrychicUI supplies content inside
  such a surface, never the surface. A component that draws its own floating
  surface is not used; one that draws an opaque card is placed in a Kit
  surface only through an adapter that clears the card. The findings per
  component are in [Floating surfaces](#floating-surfaces).
- **Only verified components.** A component is used once it is in the lists
  below with a status that allows it. The `diagram` package links pele-go:
  only `diagram.Mermaid` is used, through its adapter ([Diagrams](#diagrams)).
- **Sound is off.** `crychic.Setup()` (`internal/desktop/kit/crychic`), called
  once in `cmd/sailry-desktop` before the first window, mutes the cue the
  library plays on every interaction. A muted cue returns before a sound is
  rendered or the audio output is started. Never call `sound.SetEnabled`,
  `sound.Play*` or `sound.SetVolume` elsewhere.
- **Every copy option comes from i18n.** An empty option falls back to the
  library's English. Fill every field in the Copy column from
  `internal/desktop/i18n`. Fields marked *format* are `fmt` formats: the
  translation must keep the verbs shown, in that order.
- **Data Sailry does not author goes through the adapter.** The library panics
  on input it calls a programmer error, which includes empty lists, empty
  names, shares outside 0 to 1 and repeated IDs. Components marked *adapter*
  are called as `crychic.<Name>` with the same arguments. An adapter repairs
  what has one honest reading and otherwise builds nothing: it then returns
  `false` and MyGo's absent element, whose methods do nothing. Components
  marked *direct* are called from their own package and get no wrapper.
- **Shares** (`media.Recorder`'s level and any other 0 to 1 argument of a direct
  component) pass through `crychic.Unit`.

Status: **direct** (call the library), **adapter** (call `crychic.<Name>`),
**hosted** (direct on a page; inside a Kit floating surface only through the
adapter), **blocked** (English with no option; usable once the library takes
one), **not usable** (draws a floating surface of its own).

## What holds for every verified component

Verified by `TestApprovedComponents` and `TestPolicy` in
`internal/desktop/kit/crychic` unless marked *(read)*: read in the library's
source, not exercised.

- **Theme.** Each builds in the application theme (`theme.Apply`), light and
  dark. Colours are `c.Theme()` tokens and their mixes *(read)*; the exceptions
  are black shadows and scrims (`kit.Shade`): dialog backdrops at 0.4 as
  `Palette.Backdrop` and the Lightbox scrim at 0.88 in both appearances. Filled actions use `Accent`, which Sailry's theme
  maps to Kit's primary, and several buttons are pills (radius 999) where
  Kit's are not.
- **Surfaces.** Cards and fields are opaque `t.Background` with a 1 DIP
  border, not Kit's frosted cards. On a page that is their look. In or as a
  floating surface see [Floating surfaces](#floating-surfaces).
- **Tooltips.** `color.SwatchGrid`, `files.FileCard`, `agent.Citation`,
  `media.Recorder` and `navigation.ZoomControl` name their icon buttons with
  MyGo's own `Element.Tooltip`: a small opaque tip in the theme's inverse
  colors, not Kit's frosted `kit.Tooltip`, and shown even where Kit hides
  tips *(read)*. There is no option to turn it off.
- **Idle.** At rest none builds, paints or asks for a frame. The states below
  move by design while visible and stop in a hidden window; everything else
  animates only on arrival, hover, press or a value change.

  | Component | Moves while |
  |---|---|
  | `feedback.SaveStatus` | `saving` is true (spinner, every frame) |
  | `feedback.ConnectionStatus` | the state is `Reconnecting` (pulse) |
  | `feedback.RetryIn` | waiting: one build a second until `at` |
  | `agent.ImageGenerating` | `progress < 1` (sheen); still with Reduce Motion *(read)* |
  | `agent.TaskProgress` | `current < len(steps)` (spinner) |
  | `agent.RunTimeline` | a step is `StepRunning` (spinner) |
  | `media.Recorder` | the state is `Recording` (pulse, one build a second) |
  | `media.AudioPlayer` | `*playing` (every frame) |
  | `files.Dropzone` | a file drag is over it *(read; the tester has no drag-over)* |

  `media.Waveform` does not loop: it grows once on arrival and eases `played`.
  `diagram.Mermaid` lays its source out during a build, on the UI goroutine,
  when it first builds and when its source, font, text size or colors change.
- **Logs.** Components log state changes through the library's `kit.Log`, and
  each cue is logged by `sound`, all with `slog.Debug` under the `crychic: `
  prefix, with counts and indexes but no typed text *(read)*. Sailry's handler
  hands them to the standard handler, which drops debug records, so nothing
  reaches the log files. `sound` logs at warn or info only after a cue has
  played, which Setup prevents.

## Floating surfaces

Each frame was drawn over a red page and over a blue one, in the light
appearance, and a pixel of the surface compared (`TestPolicy`, "floating
surfaces stay Kit's frost"). A surface that hides the page gives the same
pixel twice; Kit's frost does not.

| Component | The surface it draws | Measured | Can Kit's surface host its content | Status |
|---|---|---|---|---|
| `overlay.DangerDialog` | MyGo `DialogBase` with its own panel: `t.Background`, 1.5 radius, a black shadow, over a black backdrop at 0.4 | hides the page | No. The content is built inside that panel and is not offered apart from it | **not usable** |
| `overlay.ShortcutSheet` | the same panel | hides the page | No, as above | **not usable** |
| `overlay.WhatsNew` | the same panel | hides the page | No, as above | **not usable** |
| `overlay.HoverCard` | its own `ui.Overlay` card: `t.Background`, a border, a black shadow | hides the page | Nothing to host: the content is the caller's, the library adds only the card and a 300 ms grace | **not usable** |
| `overlay.Lightbox` | no panel: a black scrim at 0.88 over the window in both appearances, with its buttons on it | an eighth of the page shows | No. Images and buttons are built on the scrim | **not usable** |
| `desktop.UpdatePrompt` | not floating itself: an opaque card (`t.Background`, a border, a faint shadow) | in `kit.Dialog` the card hides the frost; through `crychic.UpdatePrompt` the frost shows | Yes, through the adapter, which clears the card's fill and edge | **hosted** |
| `color.Picker` | the trigger is a field; its popover is MyGo's themed `ui.Popover`: `t.Background`, a border, a black shadow | hides the page | No. The library opens the popover itself | **not usable** |
| `files.FileCard` | the card is on the page; its ⋯ is MyGo's menu button, which Sailry's windows show as the system's menu | the menu opens with the caller's three items | Not a drawn surface | direct, see below |

What to use instead, with the surface Kit's:

- A destructive confirmation: `kit.Dialog` with `kit.Danger`. Where the name
  must be typed, `form.TypeToConfirm` inside `kit.Dialog` (measured: the frost
  shows around and inside it).
- A shortcut sheet or a what's-new pager: `kit.Dialog` or `kit.BareDialog`
  with the caller's content.
- A hover card: MyGo's `TooltipBase` with `kit.Floating`, as
  `kit.SessionCard` is built.
- Images: `kit.ImageViewer`, which pages through several images and also
  zooms, pans, rotates, copies and saves.
- A free color: MyGo's `ui.ColorPicker` inside `kit.Popover`.
- `crychic.UpdatePrompt` builds the prompt under a theme whose `Background`
  and `Border` are clear, so the card and the Later button's outline are gone
  and the surface it sits in is the only one. The card's shadow is in the
  text color at 0.06 and cannot be cleared: a faint line stays where the
  card's edge was. Build nothing of Kit's inside it. On a page call
  `desktop.UpdatePrompt` itself.
- `files.FileCard`'s ⋯ opens the system's menu on a left click, which
  [AGENTS.md](../AGENTS.md) keeps for right clicks. The card has no option to
  leave the button out: use it only where that is accepted, and never for a
  list that already has a Kit row menu.
- An inline component with no card of its own may sit in a Kit surface as it
  is: `form.TypeToConfirm` was measured, the `feedback` components and
  `input.TagInput` are the same by reading. One whose
  root is an opaque card (`files.FileCard`, `files.FileList`,
  `chat.LinkPreview`, `code.BranchPicker`, `devtools.LogView`,
  `agent.RunTimeline`, `agent.TaskProgress`) shows as a card on the frost;
  the surface around it stays Kit's.

## Verified components

### Appearance

| Component | Signature | Panics on | Copy | Status |
|---|---|---|---|---|
| `color.Picker` | `(c, label string, value *ui.Color) ui.Element` | empty label, nil value | none of its own; shows the hex; the popover is MyGo's `ui.ColorPicker` | **not usable** (its popover) |
| `color.SwatchGrid` | `(c, label, colors []ui.Color, value *ui.Color, o SwatchGridOptions) ui.Element` | empty label, no colours, nil value, `Columns < 0`, `Names` not one per colour | `Names` (else each hex) | direct |
| `color.Palette` | `(c, base ui.Color, o PaletteOptions) ui.Element` | `Steps` outside 2 to 20 (0 is 9) | none | direct |

### Files

| Component | Signature | Panics on | Copy | Status |
|---|---|---|---|---|
| `files.Dropzone` | `(c, label string, paths *[]string, o DropzoneOptions) ui.Element` | empty label, nil paths, negative or non-finite `Height` | `Prompt`, `Detail`, `One`, `Many` (*format* `%d`) | direct |
| `form.FileField` | `(c, label string, path *string, o FileFieldOptions) (root, browse ui.Element)` | empty label, nil path | `Empty`, `Browse`, `Remove` | direct |
| `files.FileCard` | `(c, item FileItem, action *FileAction, o FileCardOptions) (root, menu ui.Element)` | empty `Name`, negative `Size`, nil or unknown action | `Menu`, `Open`, `Rename`, `Delete`, and `FormatDate` (the default is English `Jan 2, 2006`); **fixed** size units | direct, with its menu and units |
| `files.FileIcon` | `(c, name string, o FileIconOptions) ui.Element` | empty name; a negative or non-finite `Size` | none | direct |
| `files.FileList` | `(c, label string, list []FileItem, selected *int, sort *FileSort, o FileListOptions) ui.Element` | empty label; an empty list; an entry without a name or with a negative size; two entries with the same `Path` and `Name`; nil `selected` or `sort`; a selection outside -1 to `len(list)-1`; an unknown sort | `Name`, `Size`, `Date`, and `FormatDate` (the default is English); **fixed** size units | **not for the picker**; no adapter |

- `Dropzone` appends every drop, repeats included. `FileField` keeps the first
  dropped file; `browse` only reports the click, the caller opens the dialog.
- `FileCard`'s ⋯ opens the system's menu
  ([Floating surfaces](#floating-surfaces)).
- Sizes in `FileCard` and `FileList` come from the library's
  `typography.Bytes`: decimal (1000) with the symbols `B`, `KB`, `MB`, `GB`,
  `TB` in every language. Sailry's own sizes are binary and translated
  (`KiB`, `MiB`). Do not show both for the same files.
- `FileIcon` is a tile tinted by kind (48 DIP unless `Size` says otherwise)
  over a pill with the extension in capitals. The kind comes from a fixed
  table of extensions; everything else is a grey file. A dotfile's whole name
  is its extension (`.gitignore` shows `GITIGNORE`) and a name without one
  has no pill. It is a column, so it suits a grid cell, not a 28 DIP row. A
  directory entry always has a name; check one that comes from a model.
- `FileList` is a catalog list and not a file picker. An empty directory
  panics, so the caller must show its own empty state instead. What it lacks
  against the picker's `ui.Table`:
  - activation: a click selects, a click on the selected row does nothing,
    and there is no double click or Enter;
  - a context menu, multiple selection and disabled or dimmed rows;
  - virtualisation: every row is built on every build;
  - descending order: a header click only picks the column, ascending;
    folders always sort first and names compare bytewise, so `Zed` comes
    before `apple`;
  - a Kind column, column resizing, and `Modified` cannot be left out,
    though a listing may not carry it.

  It adds a tinted kind tile per row. Its root is an opaque card.

### Forms

| Component | Signature | Panics on | Copy | Status |
|---|---|---|---|---|
| `input.KeyValueField` | `(c, rows *[]KeyValueRow, o KeyValueFieldOptions) ui.Element` | nil rows | `Add`, `Key`, `Value`; **fixed** accessible name `Remove row` | **blocked** |
| `input.PasswordField` | `(c, label string, value *string, o PasswordFieldOptions) ui.Element` | nil value | `Placeholder`; **fixed** accessible name `Reveal <label>` | **blocked** |
| `input.TagInput` | `(c, tags *[]string, draft *string, o TagInputOptions) ui.Element` | nil tags or draft | `Placeholder` | direct |

- The two fixed strings are not painted; a screen reader says them in English
  in every language. They are blocked until the library takes them as options.
- `PasswordField` always shows a four-bar strength meter, which is wrong for a
  token or an API key; settings rows keep `kit.SecretRow`.
- `TagInput` adds on Enter or a comma, removes the last tag on Backspace in an
  empty draft, has no per-chip remove and does not refuse repeats.

### Recovery

| Component | Signature | Panics on | Copy | Status |
|---|---|---|---|---|
| `feedback.ErrorState` | `(c, message string, o ErrorStateOptions) (root, retry ui.Element)` | nothing | `Title`, `Retry` | direct |
| `feedback.RetryIn` | `(c, at time.Time, o RetryInOptions) ui.Element` | nothing | `Wait` (*format* `%d` seconds), `Retry` | direct |
| `feedback.SaveStatus` | `(c, saving bool, o SaveStatusOptions) ui.Element` | nothing | `Saving`, `Saved` | direct |
| `feedback.ConnectionStatus` | `(c, state Connection, o ConnectionStatusOptions) ui.Element` | an unknown state | `Online`, `Reconnecting`, `Offline` | direct |

- `ErrorState` is for content that failed to load. An operation's outcome stays
  a toast, and `kit.Error`, `kit.Retry` and `kit.Result` win where they fit.
- `SaveStatus` shows a draft's autosave state, never the outcome of a save the
  user asked for.
- `RetryIn`'s result is the button: read `Clicked` on it.
- `ConnectionStatus` is the host page's status beside the host's name
  (`host/page.go`, [host.md](host.md)): `Host.Connection` gives one of the
  three states, never another, and the three texts are `live_connected`,
  `connection_connecting` and `connections_offline`. Kit has tags and badges
  but no status dot that pulses while a connection is coming up. It sits on
  the page, not in a floating surface, and needs no adapter.
- `ConnectionStatus` is also the status of each row in Settings ›
  Connections (`settings/connections.go`, [settings.md](settings.md)), in
  place of `kit.StatusDot` and a text: `Online` for a device that is online
  and `Offline` otherwise, never `Reconnecting`, so it is still. The three
  texts are all the row's own status (this device, peer to peer, relay,
  offline), and the element has the status role the dot and text lacked.

### Destructive confirmation

| Component | Signature | Panics on | Copy | Status |
|---|---|---|---|---|
| `form.TypeToConfirm` | `(c, name string, typed *string, o TypeToConfirmOptions) (root, confirm ui.Element)` | empty name, nil typed | `Prompt` (*format* `%s` the name), `Action` | direct |
| `overlay.DangerDialog` | `(c, title, text string, open *bool, o DangerDialogOptions) (root, confirm ui.Element)` | empty title, nil open | `Cancel`, `Confirm` | **not usable** (its panel) |

- `TypeToConfirm` matches exactly, case and spaces included. A target whose
  name can be empty (an untitled session) is confirmed another way: do not
  call it with an empty name.
- `TypeToConfirm` has no surface of its own: inside `kit.Dialog` the frost
  stays. That is the destructive confirmation with a typed name.
- `DangerDialog` draws its own opaque panel. Confirm in `kit.Dialog` with
  `kit.Danger`, naming the target and the consequence once.

### Conversation

| Component | Signature | Panics on | Copy | Status |
|---|---|---|---|---|
| `agent.ImageGenerating` | `(c, prompt string, progress float64) ui.Element` | empty prompt, progress outside 0 to 1 or NaN | none | adapter |
| `agent.SourceCard` | `(c, n int, src Source) ui.Element` | `n < 1`, empty `Title`, `Host` or `URL` | none | adapter |
| `agent.Citation` | `(c, n int, src Source) ui.Element` | as `SourceCard` | none | adapter |
| `agent.RunTimeline` | `(c, label string, steps []RunStep, o RunTimelineOptions) ui.Element` | empty label, no steps, an unnamed step, a negative duration, an unknown state, a total of zero | `Format` (*format* `%d` steps, `%s` the total as `m:ss`) | adapter |
| `agent.TaskProgress` | `(c, label string, steps []string, current int, o TaskProgressOptions) ui.Element` | empty label, no steps, an empty step, current outside 0 to `len(steps)` | `Format` (*format* `%d` step, `%d` count), `Finished` | adapter |
| `chat.LinkPreview` | `(c, title, host string, image *ui.Bitmap) ui.Element` | empty title or host | none | adapter |
| `chat.JumpToLatest` | `(c, count int, o JumpToLatestOptions) ui.Element` | a negative count | `One`, `Many` (*format* `%d`) | direct |
| `overlay.Lightbox` | `(c, label string, images []*ui.Bitmap, captions []string, at *int, open *bool, o LightboxOptions) ui.Element` | empty label, no image, a nil image, nil or out-of-range `at`, nil open, captions not one per image | `Back`, `Forth`, `Close` | **not usable** (its scrim; Kit's viewer does more) |

- Adapters: `ImageGenerating` clamps the progress and builds nothing without a
  prompt. `SourceCard` and `Citation` read a missing host from the address and
  title an untitled source by its host; no address builds nothing.
  `RunTimeline` drops unnamed steps and builds nothing until a step has taken
  time. `TaskProgress` shows an unnamed step as `…` and clamps `current`.
  `LinkPreview` titles an untitled page by its host. `Lightbox` clamps `at`,
  drops mismatched captions, and closes and builds nothing when an image is
  missing.
- `SourceCard` and `Citation` open the address themselves through
  `c.OpenURL` when `ui.SafeToOpen` allows it (http, https, mailto) and report
  the click as `Changed`.
- `SourceCard` is not used: the web search result dialog keeps its numbered
  `ui.Link` rows, one source a line (the user's decision of 2026-11-12: the
  sources are a folded tool's detail and need no cards).
- **Used beside ours:** `feedback.ConnectionStatus` for the host page's
  status and the rows of Settings › Connections.
- **Ours stays:** the picker's list (`files.FileList` loses the functions
  listed above), every Kit overlay, the image viewer, the updater, settings
  rows and editors, the composer and conversation components, the
  conversation's chart painter ([Charts](#charts)) and the host monitor's
  (`host/charts.go`), both on MyGo's `ui/chart`.

### Declined, page by page

Each place the app could take a component was read against ours. These stay
ours; the right column is the function the replacement would lose. The
components marked *(read)* are not verified in this document: they were
judged from their source alone and would need the steps under
[Adding a component](#adding-a-component) before any use.

| Component | Place | What it would lose |
|---|---|---|
| `feedback.EmptyState` *(read)* | `kit.Result` on every empty page | Our own icons (it takes a Lucide name), the second action, the dashed card and the error status; it panics on an empty title |
| `feedback.ErrorState` | `kit.Result`'s error status | It always shows Retry, shakes and repeats what the toast said |
| `feedback.Skeleton` family *(read)* | `kit.Skeleton` | The still state under Reduce Motion and the capped frame rate (its sheen asks for every frame), the sizes and the secondary tone |
| `feedback.ProgressBar`, `QuotaBar`, `StorageBar` *(read)* | Usage, storage, downloads | MyGo's `ui.Progress` comes first. `QuotaBar` panics on a limit of zero, which a provider may report, and formats raw numbers itself |
| `display.StatusBadge`, `Badge`, `Tag` *(read)* | `kit.Badge`, `kit.Tag` | Kit has them; theirs arrive with a motion and `StatusBadge` pulses without end in two states only |
| `display.Presence` *(read)*, `feedback.ConnectionStatus` | Connector and provider statuses | The failed state in the destructive colour and the spinner while a command runs; `Reconnecting` pulses without end, wrong for a warning that does not change |
| `display.Stat`, `StatCard`, `KPI` *(read)* | Host facts, usage totals | They take a `float64`; ours are formatted texts (uptime, sizes, token counts by locale) |
| `display.DescriptionList` *(read)* | `settings/details.go` facts, notification details | Values that can be selected and copied, and a key column sized by its content (theirs is fixed at 10 rem); notifications use `kit.Record` |
| `display.Timeline` *(read)* | none | No timeline is drawn by hand anywhere |
| `typography.Kbd`, `KeyCombo`, `navigation.KeyHints` *(read)* | Settings › Shortcuts, menu shortcuts | Ours is the recorder button and the platform's chord text; there is no key hint footer to fill |
| `display.Avatar`, `typography.Monogram` *(read)* | Project and session marks | `kit.Identicon` draws a mark from the ID; the provider badge belongs to the model picker, which is approved |
| `color.SwatchGrid` | The project colour popover | The accepted single row of 11 round 16 DIP swatches (its squares are 32 DIPs), Kit's tooltip and the hidden tips setting |
| `files.FileIcon` | The picker's grid, the conversation's file card | It cannot show a folder, knows fewer kinds than `picker/tile.go` and decides by extension where the card has the file's `Kind`; its extension pill overflows the 104 DIP cell; `conversation.FileIcon` is also the overview's and the panel tabs' icon |
| `files.FileList` | The picker's list | As listed under [Files](#files) |
| `navigation.ZoomControl` *(read)* | The image viewer's bar | Kit's tooltips in the frosted bar, zoom about the pointer, the disabled ends |
| `form.FileField`, `files.Dropzone` | The SSH key file | Typing a path (`~/.ssh/id_ed25519`), the full path shown (it shows the base name), the disabled state while pairing |
| `input.TagInput` | none | No list of free tags is edited: shared projects are checkboxes of offered projects. It also cannot remove a chosen chip or refuse a repeat |
| `feedback.SaveStatus` | Settings saves | Nothing saves by itself; a save's outcome is a toast, and its spinner is not the approved loading mark |
| `feedback.RetryIn` | The model retry row | The Node retries by itself and gives no time; a button would do nothing |
| `form.TypeToConfirm` | Destructive dialogs | None asks for a typed name, and adding one is a second approval |
| `agent.TaskProgress`, `RunTimeline` | The composer's overview progress | The list of every step with its skipped state; its opaque card would sit in the frosted popover; the composer is approved |
| `agent.Citation` | Citations in a reply | They are numbered links inside the Markdown text (`Cite`), which wrap and select with it |
| `agent.ImageGenerating`, `chat.LinkPreview`, `media.Waveform` and `Recorder` *(read)*, `devtools.LogView`, `code.Blame` | none | The app has no such data: no generation progress, no page preview, no blame; dictation's level is a 16 DIP glyph in a button |
| `chat.JumpToLatest` | The transcript's jump button | We do not count unseen messages; ours fades with `AtEnd` |
| `desktop.UpdatePrompt` *(read)* | The updater | Ours shows notes, progress and the restart |

## Size

`go build ./cmd/sailry-desktop` on darwin/arm64, default flags:

| Build | Bytes | Delta |
|---|---|---|
| Before the dependency | 269,300,578 | |
| `crychic.Setup` and the adapters (as committed) | 270,468,466 | +1,167,888 (1.11 MiB) |
| Every approved component referenced | 271,027,026 | +1,726,448 (1.65 MiB, 0.64%) |

Measured before `code.DiffView`, `files.FileIcon` and `files.FileList` were
verified. They add no package: `code`, `files` and MyGo's `ui/editor` were
already linked.

`diagram.Mermaid` links the `diagram` package and pele-go, estimated at about
6 MB before it was approved, while limu's layout, scene and style packages
and `limumygo` are no longer linked. The table has not been measured again
since: rebuild and add the row.

The Connections rows and the search result sources add no package:
`feedback` was linked by the host page and `agent` by the adapters.

## Adding a component

1. Say what it is for: something MyGo lacks, or a component of ours it
   replaces without losing a function. Add it to the lists above.
2. Read its source and every helper it calls. Record the signature, every
   input that panics, its copy defaults and whether each has an option, what
   moves while idle, any color that is not a theme token, and every floating
   surface it draws or opens (a dialog, a popover, a menu, a tooltip).
3. English with no option blocks it, and a floating surface of its own makes
   it not usable. Report either upstream; do not patch or copy the library.
4. Add a probe to `probes` in `internal/desktop/kit/crychic/crychic_test.go`
   with every option filled, its defaults in `english`, and an `active` view if
   a state moves by design. The test must pass in both appearances. Add each
   documented panic to the cases of `TestPolicy`, and a floating-surface case
   if it is to sit in a Kit surface.
5. Add an adapter only if it can panic on data Sailry does not author, or if
   its card must be cleared inside a Kit surface, with a hostile case in
   `TestPolicy`. A component that is safe to call gets none.
6. Run with `SAILRY_SNAPSHOTS=.runtime/review/crychic-shots` and look at both
   frames. Rebuild for the size table if a new library package is linked, and
   update `third_party_licenses/` if the pin moves.
