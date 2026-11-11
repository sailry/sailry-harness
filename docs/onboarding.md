# Onboarding

The welcome guide of the first launch (the reference implementation `onboarding.rs`,
`preferences.rs` `welcome_seen`).

## Package and files

`internal/desktop/onboarding`

| File | Holds |
|---|---|
| `onboarding.go` | `Guide` (Open, Seen), `Actions`, `View`, `Width` |

The banner's colors are `theme.Welcome` (the reference's approved
peach-to-coral gradient at 135° and white over it; themes have no brand
banner token). The mark is `icons.SailryMark`
(`assets/branding/sailry-mark-white.svg`).

## Entry point

`shell.App.Welcome`. `LoadPrefs` gives `WelcomeSeen` false when there is no
preferences file (a first launch), nil for a file from before the field
(seen), and `SetPrefs` opens the guide when false; `Prefs` saves `Seen`.
In the preview the Help menu's Welcome to Sailry opens it too
(`app.welcome`). While it shows the window's commands wait, as for the
other dialogs.

## Layout

A dialog 560 wide (the window less 48 when narrower), 85% of the window
high at most, no padding, a 1 DIP border and `radius_lg`, not closed by a
click outside: the banner (34 above, 28 below, 36 aside) with the 56 DIP
white mark, the title (24 semibold) 14 under it and the description 6
under that, in white over the gradient, and a white 32 DIP close at 12 from
the top right; the providers' picker (20 above, 26 below, 28 aside), which
ends the guide. It has no diagnostics note and no Settings button
([removed-ui.md](removed-ui.md)).

## Actions

| Action | Result |
|---|---|
| A provider or login | the guide closes as seen; Settings › Providers opens with the add editor on that choice (`settings.ProviderPicker`) |
| The editor's Back on its first step | the editor closes and the guide comes back (`ReturnTo`, flow.rs `Origin::Welcome`) |
| Close, Escape | closed, seen |

## Wiring a backend

The B4 live adapter loads the picker's API-key entries from `ListChannelPresets`.
Choices open the real Providers editor, which keeps its stable provider ID,
draft, key input and captured revision until `SaveProvider` succeeds. An
unavailable Node makes a choice toast `onboarding_host_unavailable`. OAuth
package workflows retain their B8 ownership.

`internal/desktop/preferences` owns `welcome_seen` in
`<profile>/desktop/preferences.json`. The shared Store synchronizes it across
windows. `Guide.Open` remains local; closing, Escape or continuing to Providers
marks the profile seen, while Back restores the guide. Preview retains its
isolated brands and UI preference file.

## UI state vs backend data

`Guide.Open` is UI state; `Guide.Seen` is the preference.

## Tests

`internal/desktop/shell/welcome_test.go`: a first launch asks for the guide and an
older file does not; a provider continues in Providers with the editor;
Back reopens the guide; Seen is saved. `TestSelection` covers the system
shortcuts' dialog.

## Deviations

| Deviation | Reason |
|---|---|
| Help › Welcome to Sailry | The preview's way to the guide; the reference opens it only at a first launch |
| Preview lists its own brands; live offers Node API-key presets | OAuth workflows belong to B8 |
