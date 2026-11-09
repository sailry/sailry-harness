# Onboarding

The welcome guide of the first launch (sailry-public `onboarding.rs`,
`preferences.rs` `welcome_seen`).

## Package and files

`internal/onboarding`

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
the top right; the providers' picker (20 above, 26 below, 28 aside); the
diagnostics note and Settings, centered.

## Actions

| Action | Result |
|---|---|
| A provider or login | the guide closes as seen; Settings › Providers opens with the add editor on that choice (`settings.ProviderPicker`) |
| The editor's Back on its first step | the editor closes and the guide comes back (`ReturnTo`, flow.rs `Origin::Welcome`) |
| Settings | Settings › General, where diagnostics are |
| Close, Escape | closed, seen |

## Wiring a backend

Load the picker's entries from `ListChannelPresets` as the reference does
(`Guide::load`); without a host, a choice toasts `onboarding_host_unavailable`.
`Seen` belongs in the app preferences beside the rest of `Prefs`.

## UI state vs backend data

`Guide.Open` is UI state; `Guide.Seen` is the preference.

## Tests

`internal/shell/welcome_test.go`: a first launch asks for the guide and an
older file does not; a provider continues in Providers with the editor;
Back reopens the guide; Seen is saved. `TestSelection` covers the system
shortcuts' dialog.

## Deviations

| Deviation | Reason |
|---|---|
| Help › Welcome to Sailry | The preview's way to the guide; the reference opens it only at a first launch |
| The picker lists the preview's brands | The channel presets come from the Node |
