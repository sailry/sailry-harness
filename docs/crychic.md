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
- `JumpToLatest` builds an empty box at zero and does not scroll: read
  `Clicked`.
- `Lightbox` has nothing `kit.ImageViewer` lacks. Its adapter is kept and
  tested, unused.

### Other

| Component | Signature | Panics on | Copy | Status |
|---|---|---|---|---|
| `overlay.ShortcutSheet` | `(c, title string, groups []ShortcutGroup, open *bool, o ShortcutSheetOptions) ui.Element` | empty title, no group, an untitled or empty group, a shortcut without label or keys, nil open | `Close` | **not usable** (its panel) |
| `overlay.WhatsNew` | `(c, title string, features []Feature, page *int, open *bool, o WhatsNewOptions) ui.Element` | empty title, no feature, an untitled feature, an `Icon` that is not a Lucide name, nil or out-of-range page, nil open | `Back`, `Next`, `Done` | **not usable** (its panel) |
| `desktop.UpdatePrompt` | `(c, app, version string, notes []string, o UpdatePromptOptions) (root, install, later ui.Element)` | blank app or version, an `Icon` that is not a Lucide name | `Title` (*format* `%s` the app), `Version` (*format* `%s`), `Install`, `Later` | **hosted** |
| `overlay.HoverCard` | `(c, label string, anchor ui.Element, content func()) ui.Element` | empty label, nil content | none | **not usable** (its card) |
| `navigation.ZoomControl` | `(c, label string, zoom *float64, o ZoomControlOptions) ui.Element` | empty label, nil zoom, a zoom that is NaN or outside `Min` to `Max`, an invalid range | `Out`, `In`, `Fit` | adapter |
| `media.Waveform` | `(c, label string, levels []float64, played float64, o WaveformOptions) ui.Element` | empty label, no level, a level or `played` outside 0 to 1 or NaN, a negative or non-finite `Height` | none | adapter |
| `media.Recorder` | `(c, label string, state *Take, level float64, o RecorderOptions) ui.Element` | empty label, nil or unknown state, a level outside 0 to 1 or NaN | `Record`, `Pause`, `Resume`, `Stop` | direct, level through `crychic.Unit` |
| `media.AudioPlayer` | `(c, label string, clip Clip, pos *time.Duration, playing *bool, o AudioPlayerOptions) ui.Element` | empty label, no note, a note outside 20 to 20000 Hz, a beat of zero, nil or out-of-range pos, nil playing | `Play`, `Seek` | **blocked** |
| `devtools.LogView` | `(c, lines []LogLine, query *string, follow *bool, o LogViewOptions) ui.Element` | nil query or follow, a negative `Height`, a line without time or message, an unknown level, a blank or repeated ID | `Title`, `Search`, `Follow`, `Empty`, `Levels` | adapter |
| `devtools.LevelFilter` | `(c, on *[4]bool, counts [4]int, o LevelFilterOptions) ui.Element` | nil `on`, a negative count | `Levels` | direct |
| `code.CommitGraph` | `(c, label string, commits []GraphCommit) ui.Element` | empty label, no commit, an empty hash or message, a lane outside 0 to 7, a repeated hash, a parent listed at or before its child | none | adapter |
| `code.DiffView` | `(c, st *editor.DiffState, split *bool, o DiffViewOptions) ui.Element` | nil state or flag; a negative or non-finite `Height` | `Name` (the file; the default is English `Changes`), `Split`, `Unified` | direct |
| `code.BranchPicker` | `(c, label string, branches []Branch, current *int, query *string, o BranchPickerOptions) (root, field ui.Element)` | empty label, no branch, an unnamed or repeated branch, a negative count, nil or out-of-range current, nil query | `Placeholder`, `Empty`, `Current`; **fixed** accessible name `Clear search` | **blocked** |
| `code.Blame` | `(c, label string, lines []BlameLine, o BlameOptions) ui.Element` | empty label, no line, a line without author or time | `FormatAge` (the default is English: `now`, `5m`, `2mo`) | adapter |

- `media.AudioPlayer` is blocked for what it is: it plays a list of
  synthesized notes through the cue system that Setup mutes, and cannot play an
  audio file or stream. Read-aloud and attachments keep `internal/desktop/reader`.
- `media.Recorder` is controls only. It captures nothing, and its elapsed time
  lives in the element: it resets if the element leaves the tree.
- `code.BranchPicker` is blocked by the fixed name on the clear button of the
  library's search field, shown once the query is not empty. Its adapter is
  kept and tested for when that is solved.
- `ShortcutSheet` has no adapter: its panel is not Kit's frost and nothing
  else of it can be hosted, so no page may call it.
- Adapters: `ZoomControl` clamps a stored zoom; the range in `o` is the caller's.
  `Waveform` clamps levels. `LogView` drops records without a message, shows a
  dash for a missing time, clamps levels and replaces blank or repeated IDs.
  `CommitGraph` shows the hash for a missing message, folds lanes onto eight
  and builds nothing for a history it cannot draw. `BranchPicker` and `Blame`
  build nothing for an unnamed or repeated branch and for a line without a time.
- `UpdatePrompt` takes bundled data and icons: a wrong icon name panics on
  first build, so it is caught by the feature's own test. It is a card with
  Install and Later, not a dialog, and reports only the two clicks. The
  updater's own surface, with its check, download, verification and restart,
  does more and stays.
- `Blame` builds every row and `LogView` filters every record on each build:
  give them a bounded page, not a whole history or log.
- `Blame` highlights with MyGo's editor language named by `Lang`; an unknown
  name leaves the text plain.

### Diagrams

| Component | Signature | Panics on | Copy | Status |
|---|---|---|---|---|
| `diagram.Mermaid` | `(c, source string, o MermaidOptions) ui.Element` | nothing: a source it cannot draw shows an error box | `Label` (the accessible name; else the source's `accTitle`, else English `Diagram`), `Failed` (the error box's title; else English `Diagram failed`). The box's message is pele-go's, in English | adapter |

It draws a Mermaid source natively, with no WebView and no JavaScript:
`github.com/Broderick-Westrope/pele-go` v0.2.0 (MIT, a pure Go port of Pele
1.10.0, standard library only) lays the source out and returns SVG, which the
component reads into shapes and text runs and paints itself. All 33 of
pele-go's kinds are registered: flowchart, sequence, class, state, er, gantt,
gitGraph, journey, mindmap, timeline, quadrantChart, sankey, requirement, c4,
block, architecture, kanban, packet, radar, treemap, venn, usecase, swimlane,
wardley, treeView, railroad, ishikawa, eventmodeling, cynefin, agentflow,
info, pie and xychart. The conversation uses it for every kind but the two
data charts ([conversation.md](conversation.md#b10-visuals)), in place of
limu's renderer and its MyGo adapter (`limumygo`), which are no longer
linked. limu itself stays for its parser alone: the conversation's chart
painter reads an `xychart-beta` or `pie` source's data with it.

- **Adapter.** `crychic.Mermaid(c, source, o) (ui.Element, bool)` builds
  nothing, and returns false, for a source the component would answer with
  its error box, so the caller shows the source its own way: a transcript
  shows the code block and never a red box. The component gives no result to
  ask, so `crychic.MermaidOf(c, source)` renders the source with pele-go
  first, with the component's own options and measurer, and keeps what it
  learns per source, font and text size (at most 256): whether it draws, the
  size it draws at, its kind, its `accTitle` and whether it draws a title of
  its own. A caller sizes its layout from that. This is the one place Sailry
  imports pele-go itself.
- **What fails.** A source pele-go rejects (a syntax error, an unknown kind,
  more than 50,000 characters or 5,000 edges), and a drawing the component
  cannot read. pele-go passes the value of a `style` or `classDef` property
  through as written, and the component fails the whole drawing on a `var()`
  of a custom property in pele-go's own namespaces that it does not know, so
  the adapter refuses a source that names one (`var(--_…)`, `var(--pele-…)`).
  No other markup of a model's source reaches the reader: pele-go keeps
  style properties to its own list and draws no links or images (both are
  switched off).
- **Name.** The adapter fills an empty `Label` with the source's `accTitle`,
  else with the kind, so the library's English `Diagram` is never read out.
  Give `Failed` from i18n all the same.
- **Size.** The picture fills the width it is given up to its natural width
  at the theme's text size, keeps its proportions and never grows past it.
  The component passes pele-go no width, so the layout does not depend on the
  room: the caller scales the picture or scrolls it.
- **Colors.** `Text` for text and lines, `TextMuted`, `Surface` for node
  fills, `SurfaceHover` for their second tone, `Border` for outlines,
  `Background` behind edge labels (nothing is painted behind the picture),
  `Radius` for corners, `Danger`, and for series `Accent`, `Success`,
  `Warning`, `Danger`, three mixes of them and `TextMuted`, with no option.
  Under `theme.Apply` alone the node fill is the page's own color in the
  light appearance; a caller that wants another fill builds the picture
  under `ui.Themed`, as the conversation does.
- **Cost.** The layout runs on the UI goroutine inside the build, and again
  whenever the source, font, text size or a color changes, so on every theme
  switch. The scene lives in the element (`ui.Local`): an element built anew,
  as when a virtual list brings a turn back, lays out again. With the
  adapter's own render a new source is laid out twice. In the conversation's
  test a reply with one diagram was first shown in 5 to 430 ms with both
  layouts, most kinds in 20 to 130 ms and a radar slowest.
- **Arrival.** The component rises 6 DIP and fades in over 200 ms every time
  it is built anew. The adapter holds it still (no offset, full opacity), as
  a virtual list would replay it; after those 200 ms nothing asks for a
  frame.
- **Logs.** One debug record per layout, `crychic: Mermaid drew` with the
  source's length, the number of text runs and the size, or `crychic: Mermaid
  failed` with pele-go's code, line and column: never the source *(tested)*.
- **Access.** Role image with the name; no focus, no value and nothing to
  interact with. The text inside the picture is painted, not exposed.

Verified by the probe and the adapter's cases in `internal/desktop/kit/crychic`
and by `TestLiveBlocks/diagrams` in `internal/desktop/conversation`, which
draws every kind above but the two charts.

### Charts

The `chart` package is a fixed look over MyGo's own `ui/chart`. The
components that could draw a reply's Mermaid `xychart-beta` or `pie` were
read in source for the conversation's rules *(read; none was built, as none
passes)*. None is used and no adapter exists: the conversation draws both
with its own painter on `ui/chart` directly, MyGo being first in the order
of preference ([conversation.md](conversation.md#b10-visuals)). The host
monitor's area charts, sparklines and rings are drawn the same way
(`host/charts.go`, [host.md](host.md)): they are live series with gaps for
missing samples and no hover tip, which the components below do not take
(NaN panics). The rest of the package (stacked, area, radar, sankey, treemap
and the other shapes) was not assessed.

| Component | Signature | Panics on | Copy | Status |
|---|---|---|---|---|
| `chart.Bars` | `(c, label string, labels []string, vs []float64, at *int, o BarsOptions) ui.Element` | empty label, no value, an empty or repeated label, lengths that differ, NaN or Inf, an `at` outside -1 to n-1 | none | **not used** |
| `chart.HBars` | `(c, label string, labels []string, vs []float64, at *int, o HBarsOptions) ui.Element` | as `Bars` | none | **not used** |
| `chart.GroupedBars` | `(c, label string, labels []string, series []Series, at *int, o GroupedBarsOptions) ui.Element` | as `Bars`, and no series, an unnamed or repeated series name | none | **not used** |
| `chart.Line`, `chart.Crosshair` | `(c, label string, labels []string, series []Series, at *int, o …Options) ui.Element` | as `GroupedBars` | none | **not usable** (their tip) |
| `chart.Combo` | `(c, label string, labels []string, bars, line Series, at *int, o ComboOptions) ui.Element` | as `GroupedBars` | none | **not used** |
| `chart.Pie`, `chart.Donut` | `(c, label string, names []string, vs []float64, at *int, o …Options) ui.Element` | empty label, no value, an empty or repeated name, lengths that differ, NaN or Inf, a negative value, a sum of zero, an `at` out of range | `Donut`'s `Total` | **not used** |
| `chart.ChartCard` | `(c, title, value string, delta float64, o ChartCardOptions, content func()) ui.Element` | not read further | `Delta` | **not used** (a card with a title) |

What each rule of a chart in a conversation finds:

- **Hover values.** `Bars`, `HBars`, `GroupedBars`, `Combo`, `Pie` and
  `Donut` show a value only through MyGo's `Element.Tooltip`: after the
  pointer rests 600 ms, in the theme's opaque inverse colors, not Kit's
  frost. `Line` and `Crosshair` draw their own opaque bordered tip, anchored
  above the plot's top edge, outside the chart and over the text before it.
  A component that draws its own floating surface is not used.
- **Colors.** Series take the accent and the status tones (success, warning,
  danger) in turn, with no option; only substituting the whole theme changes
  them, and a chart of status colors reads as a verdict.
- **Mermaid's model does not map.** `Combo` is exactly one bar and one line
  series on two separate axes, while Mermaid mixes any number on one. No
  component takes a y range (the domain always includes zero), horizontal
  grouped bars or lines, or a missing value (NaN panics). Series need
  distinct names, which Mermaid's usually lack, and a legend is always shown
  by `GroupedBars`, `Combo`, `Pie` and `Donut`. Every category label is drawn
  whole in a zero-width box: nothing is left out or cut short.
- **Picking.** A click picks a mark through the caller's `at` (the others dim,
  a slice lifts) with no pointer way to clear it.
- **Motion.** A 700 ms arrival plays whenever the element enters the tree,
  which the virtual transcript does on every scroll back.
- **Hostile data** would need an adapter for every panic above (repeated
  labels, NaN, lengths, negative shares), after which the gaps above remain.

Using a component for the few sources it could draw, and the painter for the
rest, would give near-identical charts two looks and two hover behaviors.

### `code.CommitGraph`

It draws what it is given and computes nothing. Per commit, newest first:

| Field | Required | Meaning |
|---|---|---|
| `Hash` | yes, unique in the list | Identity, and the key parents refer to. The row shows its first 7 characters |
| `Message` | yes | The row's text and accessible name, on one line |
| `Lane` | 0 to 7 | The column of the commit's dot. The caller assigns lanes: the library has no layout of its own |
| `Parents` | no | Hashes. A parent in the list must come later in it; an edge is drawn to its dot, curving to its lane. A parent not in the list (beyond the page) is drawn as a line to the bottom edge. None draws no edge. Two or more make the dot larger |
| `Refs` | no | Branch and tag names, shown as chips in the lane's color |
| `Author` | no | Shown muted after the message |

- So a history needs each commit's parents, an order in which every child
  precedes its parents (`git log --topo-order`, or by date for an ordinary
  history), and a lane per commit worked out by the caller. A listing without
  parents cannot be drawn as a graph.
- Panics (each by test): no commits, a commit without a message, a lane of 8,
  a repeated hash, a parent listed before its child. Also an empty label or
  hash and a commit that names itself as parent *(read)*.
- `crychic.CommitGraph` shows the hash for a missing message, folds lanes
  onto the eight drawn (lane 9 is drawn in lane 1, so two branches can share
  a column) and builds nothing for a history it cannot draw: none, an
  unnamed or repeated commit, or a parent listed before its child. Check the
  result and show the plain history list instead.
- Rows are not interactive: no selection, click or menu. Lane colors repeat
  after four (accent, success, warning, danger).
- It is not virtual. 400 commits build 400 rows at 36 DIP each, 14,408 DIP
  tall, and every paint strokes every edge; that frame is at rest after the
  one-second arrival and asks for no more *(tested with 400)*. It has no
  scroll of its own: put it in a `ui.Scroll`, and page the history rather
  than give it thousands of commits.

In use: the Git history, in the panel's tab and the Git page
(`internal/desktop/panel/git_history.go`, [panel.md](panel.md#layout)).

- The Node sends each commit's parents and names `HEAD` among the references
  at its commit; `graphLanes` assigns the lanes. A lane is held for the
  commit it awaits, so every line the component draws down a lane ends at the
  next dot in it; a merge's second parent takes the first free lane, and a
  branch that joins a commit another child already awaits curves into that
  child's lane.
- A line to a parent that is not listed yet is drawn straight down the
  commit's own lane to the end of the list, so `graphLanes` keeps that lane
  for it and puts nothing in its way.
- The rows over the graph are ours: a `ui.RoleListBox` of options laid at the
  component's rows (`crychic.CommitGraphRows`, held to the component by a
  test) that take the click, the selection, the tooltip and the OS-native
  right-click menu. They take a click in the first frame: the component's
  arrival moves its own rows and draws its lines in, not ours *(tested)*.
- The selected and hovered fills (`theme.Palette.Selected()`, `Hover`) are
  painted under the graph and start after the lanes. The component paints
  the theme's background around each dot, which a fill under the dot shows
  as a ring.
- One line a commit. The component always shows the short ID first and puts
  every name it is given before the subject, shrinking the names with the
  subject when the line overflows, so the caller chooses what to give it
  (`crychic.CommitGraphLanes` and `CommitGraphPad` say what the lanes and
  padding take): under 200 DIP of text at most one name, the branch checked
  out first, with the count of the others (`+2`) when it fits; from 200 the
  names that fit in half of the line; from 340 also the author and the
  commit's age after the subject. The row's tooltip says the full subject,
  ID, author, age and names at any width.
- The Node reads the history 100 commits at a time and the list grows by
  Load more at its end, because the component draws a line only between
  commits of one list: a page of its own would cut every line that crosses
  into it. Keys are the hashes, so a longer list keeps its rows and the
  component's lines are not drawn in again; only the new rows fade in. A
  merge whose second parent is read later gets its lane then, which can
  move the rows below it one lane over.
- It is not virtual, so the cost grows with the list: a headless frame of
  the Git page takes 0.3 ms with 100 commits, 1.7 ms with 500, 3.5 ms with
  1,000 and 18 ms with 3,000 *(measured once, Apple silicon, not a test)*.
  Nothing caps the list.
- The arrival cannot be held still from outside as `crychic.Mermaid`'s is:
  it is the trim of the lines in the component's painter and a keyframe on
  each of its rows, not a pose of the root. It plays once when the history
  shows, and again each time the History tab is chosen.
- A history it cannot draw (an earlier Node that sends no parents, a parent
  listed before its child) is the plain list, with the same rows and menu.
- The lane colours are the component's, from the theme's accent, success,
  warning and danger tokens through the adapter rule; there is no page
  palette.

### `code.DiffView`

MyGo's `editor.DiffView` under a file bar: a file glyph, `Name`, `+N` and
`−N` from the state, and a Split and Unified switch.

- The caller owns the `editor.DiffState`, so the theme, font, hunk actions
  and fit set on it stay. The component writes `st.Layout` from `*split` on
  every build: the flag wins over the state *(tested)*.
- Height. As built the root is `o.Height` tall, 320 DIP by default: it stays
  320 in a taller body and shrinks into a shorter one. `Grow(1)` on the
  returned root makes it fill a flexible tab body *(tested at 200 and 600
  DIP)*; nothing in the library has to change.
- It has its own border and radius and plays an arrival. The counts in the
  bar duplicate any `+N −N` the caller shows.
- Empty, binary and partial states are not its concern: build it only for a
  text diff.

Not used. The panel's Unified and Split choice is ours: `kit.DiffLayout`, a
small `kit.Segmented` in the diff's toolbar, and `kit.DiffOptions.Split`,
which sets the same MyGo layout (`editor.DiffSplit`) this component sets.
Nothing of the viewer differs, so per-change Revert, Keep and Stage, the
selection, the highlighting, long lines and the text size are the same in
both. What decided it:

- The choice covers every diff of the window: a file's, All changes, and a
  commit's or stash's documents. The last two are several viewers as tall as
  their text in one scroll; a fixed-height bordered card with a bar and an
  arrival for each does not fit them, so `code.DiffView` could serve one of
  the three and the choice would need a second control anyway.
- Its bar repeats the file's name and `+N −N`, which the tab and the status
  bar already show, and its switch is a pair of buttons rather than the
  segmented control the header asks for.

## Compared with ours

A read-only audit compared the whole library with our components by reading
alone. Its findings about the components verified here were checked; its
findings about components not in this document were not.

| Finding | Result |
|---|---|
| CrychicUI needs MyGo v0.8.3; does it build against our pin? | Yes: the adapters and the tests build and pass on the pinned MyGo |
| Colors come from `c.Theme()`; surfaces are opaque cards | Confirmed: every probe builds in both appearances; the overlays hide the page *(tested)* |
| Every interaction asks for a cue; `SetEnabled(false)` silences it | Confirmed *(tested)*: the cue is recorded and nothing plays |
| Logs are `slog.Debug` | Confirmed *(tested)* |
| Fixed English: `input.PasswordField` (`Reveal <label>`), `input.KeyValueField` (`Remove row`), `code.BranchPicker` (`Clear search`) | Confirmed *(tested)*: each is present with every option filled |
| Sizes are `B`, `KB`, `MB`; dates and ages default to English | Confirmed *(tested)*: `KB` shows in `FileCard` and `FileList`; `FormatDate` and `FormatAge` replace the others |
| `files.FileList` panics on an empty directory and on duplicates | Confirmed *(tested)*. Corrected: a duplicate is the same `Path` and `Name`; one name in two folders is accepted |
| `files.FileIcon` panics only on an empty name | Corrected: also on a negative or non-finite `Size` *(read)* |
| `code.CommitGraph` panics on no commits, an empty message, more than 8 lanes, a child after its parent | Confirmed *(tested)*. Also a repeated or empty hash and an empty label. A parent outside the list is accepted |
| `code.BranchPicker` panics on a repository with no branches | Confirmed *(tested)* |
| `overlay.Lightbox` panics on an image not decoded | Confirmed *(tested)* |
| `devtools.LogView` panics on a line without a time or message; `overlay.ShortcutSheet` on no groups | Confirmed *(tested)* |
| `code.DiffView` has a fixed height; can it fill a tab? | It can: `Grow(1)` on the root *(tested)* |
| `code.DiffView` rewrites `st.Layout` on every build; its labels are options | Confirmed *(tested)* |
| `feedback.ErrorState` always shows Retry | Confirmed *(tested)*; it also shakes once and asks for a cue *(read)* |
| `chat.JumpToLatest` draws nothing at zero | Confirmed *(tested)*: an empty box |
| Tooltips are MyGo's own and ignore Kit's hidden tips | Confirmed *(read)* |
| Are arrivals skipped under Reduce Motion? | By MyGo's contract for `Element.Keyframes`, playback completes at once *(read, not tested)* |

### What ours keeps

The audit's decisions stand where ours does more; none was overturned here.

- **Use theirs for something we lack:** `code.Blame` and `devtools.LogView`
  when those features are added. The audit also named `files.FileIcon` for
  the picker's grid cells; reading both declined it
  ([below](#declined-page-by-page)).
- **Ours stays, with what it lacked added:** the panel's diffs keep
  `kit.Diff` and gained the Split and Unified choice themselves
  ([`code.DiffView`](#codediffview) says why not the component).
- **In use:** `code.CommitGraph` draws the Git history
  ([`code.CommitGraph`](#codecommitgraph)).
- **Replaced:** limu's renderer in the conversation by `diagram.Mermaid`,
  at the user's request. What the conversation's own wrapper did stays ours
  and is kept: code while the block streams and when it cannot be drawn, no
  card, one title, the name for assistive technology, the caps on width and
  height with the scroll past them, and no motion
  ([conversation.md](conversation.md#b10-visuals)). What limu did and the
  component does not: lay a diagram out for the reply's width, keep one
  layout across remounts, and repaint without a layout on a theme switch.
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
| `data.DataTable` *(read)* | A package's `DataTable` (`plugins/native_data.go`): the Usage page's tables, the database result grid | Row selection and its events, copying the chosen rows, the cells' system menu, column resizing, the sideways scroll under a minimum width, the fill column and cell tooltips. It sorts on a header click, plays a cue and takes plain rows only. Its look (frame, filled header, compact separated rows) is drawn around MyGo's `ui.Table` instead ([plugins.md](plugins.md#deviations)) |
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
