# Removed UI

Controls the accepted UI dropped on purpose. They must not come back.

The Rust application and the root `locales/` catalogs still carry most of
them, so a parity audit against either finds each one as a "missing"
control. It is not missing. Before adding a control found in Rust or in
another reference, read this list; a control on it stays out unless the user
asks for it by name, and then its entry here is removed in the same change.

The backend keeps the capability where one exists: the entries say what
stays. Removing a control never removes a command, a protocol field or
stored data.

## The list

Commits are in this repository. Dates are the removal's.

| # | Removed | Where it would appear | Why, and what stays | Decision |
|---|---|---|---|---|
| 1 | A host choice in the composer, and a location dialog | The composer's context bar and its compact settings popover (`composer/options.go`, `composer/composer.go`) | The project menu lists every host's projects; choosing a project sets the host. A session keeps its Node and location. `Context.Hosts` and `Host` stay as execution-owner metadata; the live window leaves `Hosts` empty ([composer.md](composer.md)) | `6dea10e`, 2026-11-09 |
| 2 | A header, footer, hint or check mark in the `@` and `/` picker | The reference picker over the input (`composer/references.go`) | The picker is a full-width list of one-line rows. Its title is the accessibility label only; group headings stay | `8299d17`, 2026-11-09 |
| 3 | A Deny menu with "Deny with reason", and the reason field | The approval popover (`composer/interactions.go`) | Deny and Allow alone; Deny calls `Decide(id, Deny, "")`. The Node still accepts `deny_with_reason` from other clients and gives the reason to the model | `aad2e14`, 2026-11-09 |
| 4 | State text on a question row, including "waiting for answer" | The conversation's question block (`conversation/tools.go`) | The row's icon carries the state | `aad2e14`, `00ca81e`, 2026-11-09 |
| 5 | Red for a tool's returned error | A tool call's row in the conversation (`conversation/tools.go`) | Only a call that failed to run is red; an error the tool returned reads plain | `9bfa306`, 2026-11-09 |
| 6 | An inline Connect button on an unconnected connector row | Settings › Connectors (`settings/connectors.go`) | Connect is in the row's menu | `d2275b5`, 2026-11-09 |
| 7 | A Revoke or Remove pairing button for a host outside a menu | A host row in Settings › Connections (`settings/connections.go`); the host page's header (`host/page.go`) | Revoke is in the row's ⋯ menu, beside Edit network. The rail's right-click host menu asks for it too and opens the host page's confirmation ([host.md](host.md)) | `5689924`, 2026-11-09 |
| 8 | The telemetry or diagnostics note, and a Settings button | The welcome guide (`onboarding/onboarding.go`) | The guide is its banner and the providers' picker. Diagnostics are in Settings › General ([onboarding.md](onboarding.md)) | `c2df963`, 2026-11-09 |
| 9 | Transparency or blur values in a theme package | A theme's manifest and tokens (`internal/desktop/theme`, `themes/`) | Themes define colours only. Transparency and blur are the Appearance page's settings (`theme.Surfaces`) | User decision, 2026-11-10 |
| 10 | The "local models" heading | Settings › Dictation (`settings/models.go`) | The model group has no heading | `45078c7`, 2026-11-09 |
| 11 | A preview note in the provider editor; Add model as a footer row of the model list | Settings › Providers, the channel editor (`settings/`) | Add model is a button beside Refresh | `831c8a6`, 2026-11-09 |
| 12 | A pane close that stops, closes or removes its session or terminal | The split workspace (`shell/panes.go`) | Closing a pane only detaches it from its split (`closePane` → `Detach`) and sends no command. A terminal pane's close confirmation stays. A terminal tab's own close in the right panel is a different control ([panel.md](panel.md)) | Split workspace decision |
| 13 | The `git` plugin's Status-slot entry | The composer's activity strip (`live/plugin_status.go`) | The Git +/− badge is native core, from core Git status. The `files` and `git` packages are not mounted in the Status slot | Phase B5 |
| 14 | The capability filters (vision, image and the others) and the line over them | The model picker's rail, under the families (`composer/models.go` `modelPicker`) | The rail is Back and a tile for each family. The capability marks at the end of a model's row stay, with their tooltips; `Model.Capabilities` and the Providers page's capability boxes stay ([composer.md](composer.md), [media.md](media.md#desktop)) | User decision, 2026-11-12 |
| 15 | A check mark on the chosen row | The model picker's effort rows, model rows and family tiles, and the team list's rows of the same popover (`composer/models.go`) | The neutral fill `Palette.Selected` alone marks the choice; each is still selected for assistive technology. The mode and permission menus keep their checks | User decision, 2026-11-12 |
| 16 | The score card or any tooltip over a score | The model picker's model rows and effort rows (`composer/models.go` `modelRow`, `effortColumn`) | The score is text at the row's end. The controls page keeps the card over its effort label (`scoreTip`), with the `model_score_*` copy | User decision, 2026-11-12 |
| 17 | The host's name on a second line under the title | The file picker's heading (`picker/view.go` `heading`) | The heading is the title alone, level with the header bar. A host other than this device is named after the sidebar's first group heading, and after the title where the sidebar gives way; this device is not named ([files.md](files.md#host-file-picker)) | User decision, 2026-11-12 |
| 18 | A wait reason on a queued row ("Held", "Paused", "Paused after stop", "Paused after interruption", "Not sent"), and held background results as rows in the queue ("Held at automatic limit", "Held after failure") | The composer's queue strip (`composer/queue.go`) | The strip lists queued input only, as plain rows. A queue the Node stopped sending is one toast when it happens (`queue_halt_interrupted`, `queue_halt_unsent`); the user's own Stop or pause is not reported. The Node and the protocol keep `pause_reason` and `hold`; a held result joins the user's next message and shows in the transcript with the turn that takes it ([composer.md](composer.md), [conversation.md](conversation.md)) | User decision, 2026-11-12 |
| 19 | The size's value beside the message text size slider, and the stepped sizes (Small, Standard, Large, Extra large) the slider replaced | Settings › Conversation (`settings/conversation.go`) | The slider alone, one tick a pixel from 12 to 18; the transcript shows the size ([conversation.md](conversation.md)) | `b612a83`, `c8f0be4`, 2026-11-12 |
| 20 | The toast that a language change applies after a restart | Settings › General (`settings/general.go`) | The window redraws in the language chosen at once, so nothing is said | `0b2988b`, 2026-11-12 |

## Guards

Tests fail when one of the cheaply checked controls returns. They are
subtests of the feature's existing tests.

| Guard | Test |
|---|---|
| The copy of removed controls is not in `keys.txt` | `i18n` `TestCatalogKeys/RemovedControls` |
| The composer builds no host control, wide or compact | `shell` `composerParity/no host control` (a step of `TestComposerGuidesRunningTurn`); `live` `testSessionForwarding` |
| The approval popover has no reason field and no Deny menu | `shell` `TestApprovalsAndQuestion/no deny reason`; `live` `interactionKinds` |
| The welcome guide has no diagnostics note and no Settings button | `shell` `TestWelcomeGuide/no diagnostics note or Settings` |
| The host page has no Remove pairing button | `shell` `testHostMenu` |
| The model picker's rail has no capability filter and no line under the families | `shell` `composerParity/model picker/rail` (`composerModels`) |
| The model picker's chosen rows draw no check | `shell` `composerParity/model picker/selection dark` and `selection light` |
| A score in the model picker opens no tooltip | `shell` `composerParity/model picker/scores open nothing` |
| The file picker's heading is one line and names no local host | `picker` `TestModel/view/heading` |
| The queue strip draws no wait reason, and no strip without queued input | `composer` `TestQueueTray`; `live` `TestQueueStates/held results are not rows`; the `queue_wait_*` keys in `i18n` `TestCatalogKeys/RemovedControls` |
| The text size row has no value and no stepped sizes; a language change says nothing of a restart | The `message_text_*` and `settings_language_restart` keys in `i18n` `TestCatalogKeys/RemovedControls` |

A key on the guard's list cannot be added to `keys.txt` without editing the
test, which points here.

## Adding an entry

When the user removes a control, add a row with what it was, where it would
appear, what stays and the commit, and a guard where one is cheap: a key
kept out of `keys.txt`, or an absence check in the feature's test.
