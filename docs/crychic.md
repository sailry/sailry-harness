# CrychicUI

`github.com/ELY-Open-Source/CrychicUI` (MIT) is a third-party MyGo component
library. Desktop links it at the pseudo-version pinned in `go.mod`
(`v0.0.0-20261110123803-1c55e6a884bb`) against Sailry's own MyGo pin; notices
are in [third_party_licenses/crychicui.md](../third_party_licenses/crychicui.md).

## Rules

- **Gap-filling only; Kit wins when both exist.** Choose a component in the
  order of [components.md](components.md#choosing): MyGo, then Kit, then this
  list. Never replace an existing Kit component, and never use a CrychicUI
  dialog, menu, toast, tabs or button where Kit has one.
- **Only the approved list below.** Not approved: `agent.Approval`, `Plan`,
  `ToolCall`, `Reasoning`, `ModelPicker`, `ContextMeter`, `data.ReorderList`
  and everything not listed. Never import `diagram`: it links pele-go and
  adds about 6 MB.
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
  on input it calls a programmer error, which includes empty names, shares
  outside 0 to 1 and repeated IDs. Components marked *adapter* are called as
  `crychic.<Name>` with the same arguments. An adapter repairs what has one
  honest reading and otherwise builds nothing: it then returns `false` and
  MyGo's absent element, whose methods do nothing. Components marked *direct*
  are called from their own package and get no wrapper.
- **Shares** (`media.Recorder`'s level and any other 0 to 1 argument of a direct
  component) pass through `crychic.Unit`.

## What holds for every approved component

Verified by `TestApprovedComponents` and `TestPolicy` in
`internal/desktop/kit/crychic` unless marked *(read)*.

- **Theme.** Each builds in the application theme (`theme.Apply`), light and
  dark. Colours are `c.Theme()` tokens and their mixes *(read)*; the exceptions
  are black shadows and scrims (`kit.Shade`): dialog backdrops at 0.4 as
  `Palette.Backdrop` and the Lightbox scrim at 0.88 in both appearances. Filled actions use `Accent`, which Sailry's theme
  maps to Kit's primary, and several buttons are pills (radius 999) where
  Kit's are not.
- **Floating layers.** `DangerDialog`, `ShortcutSheet`, `WhatsNew`, `Lightbox`,
  `HoverCard` and `Picker`'s popover set their own surface on MyGo's bases.
  They do not paint through `kit.Floating`, so they have no frost. This is the
  accepted deviation from [components.md](components.md#floating-layers-and-overlays)
  for these six only.
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
- **Logs.** Components log state changes through the library's `kit.Log`, and
  each cue is logged by `sound`, all with `slog.Debug` under the `crychic: `
  prefix, with counts and indexes but no typed text *(read)*. Sailry's handler
  hands them to the standard handler, which drops debug records, so nothing
  reaches the log files. `sound` logs at warn or info only after a cue has
  played, which Setup prevents.

## Approved components

Status: **direct** (call the library), **adapter** (call `crychic.<Name>`),
**blocked** (do not use until solved).

### Appearance

| Component | Signature | Panics on | Copy | Status |
|---|---|---|---|---|
| `color.Picker` | `(c, label string, value *ui.Color) ui.Element` | empty label, nil value | none of its own; shows the hex; the popover is MyGo's `ui.ColorPicker` | direct |
| `color.SwatchGrid` | `(c, label, colors []ui.Color, value *ui.Color, o SwatchGridOptions) ui.Element` | empty label, no colours, nil value, `Columns < 0`, `Names` not one per colour | `Names` (else each hex) | direct |
| `color.Palette` | `(c, base ui.Color, o PaletteOptions) ui.Element` | `Steps` outside 2 to 20 (0 is 9) | none | direct |

### Files

| Component | Signature | Panics on | Copy | Status |
|---|---|---|---|---|
| `files.Dropzone` | `(c, label string, paths *[]string, o DropzoneOptions) ui.Element` | empty label, nil paths, negative or non-finite `Height` | `Prompt`, `Detail`, `One`, `Many` (*format* `%d`) | direct |
| `form.FileField` | `(c, label string, path *string, o FileFieldOptions) (root, browse ui.Element)` | empty label, nil path | `Empty`, `Browse`, `Remove` | direct |
| `files.FileCard` | `(c, item FileItem, action *FileAction, o FileCardOptions) (root, menu ui.Element)` | empty `Name`, negative `Size`, nil or unknown action | `Menu`, `Open`, `Rename`, `Delete`, and `FormatDate` (the default is English `Jan 2, 2006`) | direct |

- `Dropzone` appends every drop, repeats included. `FileField` keeps the first
  dropped file; `browse` only reports the click, the caller opens the dialog.
- `FileCard`'s ellipsis opens MyGo's own menu, not `kit.Menu`. Sizes read
  `B`, `KB`, `MB` in every language.

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

### Destructive confirmation

| Component | Signature | Panics on | Copy | Status |
|---|---|---|---|---|
| `form.TypeToConfirm` | `(c, name string, typed *string, o TypeToConfirmOptions) (root, confirm ui.Element)` | empty name, nil typed | `Prompt` (*format* `%s` the name), `Action` | direct |
| `overlay.DangerDialog` | `(c, title, text string, open *bool, o DangerDialogOptions) (root, confirm ui.Element)` | empty title, nil open | `Cancel`, `Confirm` | direct |

- `TypeToConfirm` matches exactly, case and spaces included. A target whose
  name can be empty (an untitled session) is confirmed another way: do not
  call it with an empty name.
- `DangerDialog` closes itself on either button; `confirm` is the absent
  element while closed. Name the target and the consequence once, in `title`
  and `text`. A confirmation Kit's `Dialog` with `kit.Danger` already covers
  stays on Kit.

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
| `overlay.Lightbox` | `(c, label string, images []*ui.Bitmap, captions []string, at *int, open *bool, o LightboxOptions) ui.Element` | empty label, no image, a nil image, nil or out-of-range `at`, nil open, captions not one per image | `Back`, `Forth`, `Close` | adapter |

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
- `JumpToLatest` builds an empty box at zero and does not scroll: read
  `Clicked`.
- `Lightbox` overlaps Kit's image viewer; use it only for what that lacks.

### Other

| Component | Signature | Panics on | Copy | Status |
|---|---|---|---|---|
| `overlay.ShortcutSheet` | `(c, title string, groups []ShortcutGroup, open *bool, o ShortcutSheetOptions) ui.Element` | empty title, no group, an untitled or empty group, a shortcut without label or keys, nil open | `Close` | adapter |
| `overlay.WhatsNew` | `(c, title string, features []Feature, page *int, open *bool, o WhatsNewOptions) ui.Element` | empty title, no feature, an untitled feature, an `Icon` that is not a Lucide name, nil or out-of-range page, nil open | `Back`, `Next`, `Done` | direct |
| `desktop.UpdatePrompt` | `(c, app, version string, notes []string, o UpdatePromptOptions) (root, install, later ui.Element)` | blank app or version, an `Icon` that is not a Lucide name | `Title` (*format* `%s` the app), `Version` (*format* `%s`), `Install`, `Later` | direct |
| `overlay.HoverCard` | `(c, label string, anchor ui.Element, content func()) ui.Element` | empty label, nil content | none | direct |
| `navigation.ZoomControl` | `(c, label string, zoom *float64, o ZoomControlOptions) ui.Element` | empty label, nil zoom, a zoom that is NaN or outside `Min` to `Max`, an invalid range | `Out`, `In`, `Fit` | adapter |
| `media.Waveform` | `(c, label string, levels []float64, played float64, o WaveformOptions) ui.Element` | empty label, no level, a level or `played` outside 0 to 1 or NaN, a negative or non-finite `Height` | none | adapter |
| `media.Recorder` | `(c, label string, state *Take, level float64, o RecorderOptions) ui.Element` | empty label, nil or unknown state, a level outside 0 to 1 or NaN | `Record`, `Pause`, `Resume`, `Stop` | direct, level through `crychic.Unit` |
| `media.AudioPlayer` | `(c, label string, clip Clip, pos *time.Duration, playing *bool, o AudioPlayerOptions) ui.Element` | empty label, no note, a note outside 20 to 20000 Hz, a beat of zero, nil or out-of-range pos, nil playing | `Play`, `Seek` | **blocked** |
| `devtools.LogView` | `(c, lines []LogLine, query *string, follow *bool, o LogViewOptions) ui.Element` | nil query or follow, a negative `Height`, a line without time or message, an unknown level, a blank or repeated ID | `Title`, `Search`, `Follow`, `Empty`, `Levels` | adapter |
| `devtools.LevelFilter` | `(c, on *[4]bool, counts [4]int, o LevelFilterOptions) ui.Element` | nil `on`, a negative count | `Levels` | direct |
| `code.CommitGraph` | `(c, label string, commits []GraphCommit) ui.Element` | empty label, no commit, an empty hash or message, a lane outside 0 to 7, a repeated hash, a parent listed before its child | none | adapter |
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
- Adapters: `ShortcutSheet` drops actions without keys and groups left empty.
  `ZoomControl` clamps a stored zoom; the range in `o` is the caller's.
  `Waveform` clamps levels. `LogView` drops records without a message, shows a
  dash for a missing time, clamps levels and replaces blank or repeated IDs.
  `CommitGraph` shows the hash for a missing message, folds lanes onto eight
  and builds nothing for a history it cannot draw. `BranchPicker` and `Blame`
  build nothing for an unnamed or repeated branch and for a line without a time.
- `WhatsNew` and `UpdatePrompt` take bundled data and icons: a wrong icon name
  panics on first build, so it is caught by the feature's own test.
  `UpdatePrompt` is a card, not a dialog; the updater's existing surface wins
  where it already shows the same thing.
- `HoverCard` stays open for 300 ms after the pointer leaves, and opens on
  keyboard focus of the anchor.
- `CommitGraph` and `Blame` build every row and `LogView` filters every record
  on each build: give them a bounded page, not a whole history or log.
- `Blame` highlights with MyGo's editor language named by `Lang`; an unknown
  name leaves the text plain.

## Size

`go build ./cmd/sailry-desktop` on darwin/arm64, default flags:

| Build | Bytes | Delta |
|---|---|---|
| Before the dependency | 269,300,578 | |
| `crychic.Setup` and the adapters (as committed) | 270,468,466 | +1,167,888 (1.11 MiB) |
| Every approved component referenced | 271,027,026 | +1,726,448 (1.65 MiB, 0.64%) |

## Adding a component

1. Show the gap: neither MyGo nor Kit has it. Get it approved and add it to
   the list above with its use.
2. Read its source and every helper it calls. Record the signature, every
   input that panics, its copy defaults and whether each has an option, what
   moves while idle, and any colour that is not a theme token.
3. English with no option blocks it. Report it upstream; do not patch or copy
   the library.
4. Add a probe to `probes` in `internal/desktop/kit/crychic/crychic_test.go`
   with every option filled, its defaults in `english`, and an `active` view if
   a state moves by design. The test must pass in both appearances.
5. Add an adapter only if it can panic on data Sailry does not author, with a
   hostile case in `TestPolicy`. A component that is safe to call gets none.
6. Run with `SAILRY_SNAPSHOTS=.runtime/review/crychic-shots` and look at both
   frames. Rebuild for the size table if a new library package is linked, and
   update `third_party_licenses/` if the pin moves.
