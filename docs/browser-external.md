# Managed external browser

Node owns managed browser sessions in `internal/node/externalbrowser` and routes
them in `internal/node/external_browser.go`. The bundled `external-browser`
package declares the 49 agent tools; Node binds them. The browser is an
installed Chrome, Chromium or Edge that Node starts itself with an isolated
profile. Node never attaches to a browser the user
runs and never opens the user's browser profile. Preview data never starts a
browser.

The driver is [chromedp](https://github.com/chromedp/chromedp) v0.23.0 with
`cdproto` v0.159.0, pinned in `go.mod`. It speaks the Chrome DevTools Protocol
over the browser's own pipe (`--remote-debugging-pipe`), so no debugging port is
opened and no separate driver process runs.

## Browser

`externalbrowser.Installed` finds a Chrome, Chromium or Edge installed at the
platform's standard locations. That is the only browser ever used, and it is
only ever started with a session's own `--user-data-dir`. Node downloads no
browser. A `tools/chromium` directory left in the profile by the Rust
application is neither used nor touched.

Without an installed browser every call, whatever its action or arguments,
fails at once with `not_configured` "no supported browser is installed on the
execution Node; install Google Chrome, Chromium or Microsoft Edge". Nothing is
created under the profile and nothing is retried. The public command answers
the fault before admission; a turn's tools stay offered and answer it as their
result, so the model and the user can read it. The next call looks again, so
installing a browser needs no restart.

`Available` is false only when Node has no profile or has stopped. The tools
are not offered then.

## Sessions

One browser process tree belongs to one conversation session.

| Fact | Value |
| --- | --- |
| Sessions per Node | 4; a fifth is `busy` "close an external browser session before starting another" |
| User data directory | `<profile>/browser/sessions/<session>/profile`, private, kept between browser starts |
| Temporary files | `<profile>/tmp/browser-<session>-*` with `downloads/`, removed when the browser closes |
| Viewport | 1280 × 900, headless |
| Deadlines | 45 s per operation, 25 s page load, 15 s script |

Operations of one session run one at a time. The browser starts on the first
operation that needs it and is reused until it closes. It closes when:

- `close_session` runs;
- an operation is interrupted by its deadline, by its turn stopping or by Node
  stopping. The fault is `cancelled` "external browser call interrupted; browser
  session closed" for a read action and `outcome_unknown` "external browser call
  interrupted; effects may have occurred; browser session closed; verify before
  retrying" for a control action;
- a stopped turn ends, whichever tool it was in;
- the conversation is removed (`sessions_removed`);
- Node stops (`Browsers.Shutdown`, before the Node waits for its work).

Closing asks the browser to exit, then kills its process group (a job object on
Windows, the parent-death signal on Linux as well), waits for the process and
kills any process still using the session's user data directory. A Node that
did not stop cleanly is recovered at the next start: `New` kills processes whose
`--user-data-dir` is below `<profile>/browser/sessions/` and removes stale
`<profile>/tmp/browser-*` directories. A browser whose process exited on its
own is replaced on the next operation.

## Command

`use_external_browser {session, worktree, action, arguments}` is durable and
answers `external_browser`:

```json
{"execution_node": [...], "result": {...}, "truncated": false, "inline_data": [{"mime_type": "image/jpeg", "data": "..."}]}
```

`close_session` answers `{"closed": true, "execution_node": [...]}`, `downloads`
answers `{"execution_node", "files", "truncated"}` and `save_download` answers
`{"execution_node", "path"}`.

`Node.dispatchExternalBrowser` checks, in order: the installed browser, the
arguments
(`externalbrowser.Validate`), that the session exists and belongs to the named
worktree (`permission_denied` "resource session belongs to another worktree"),
the worktree root, and for a plugin call made for a turn that the turn is still
running. It then admits the request through the admission ledger, so a repeated
request ID replays the first outcome. The operation runs on the Node's lifetime,
or on the turn's for a plugin call made for a turn.

| Bound | Value |
| --- | --- |
| Arguments | A JSON object of at most 64 KiB |
| URLs (`navigate`, `new_tab`, `new_window`) | `http` or `https` without credentials |
| `wait` seconds | Non-negative and representable |
| Result | 48 KiB of text and 128 entries per collection; `truncated` reports a cut |
| Upload, saved download | 10 MiB |
| Saved capture or PDF | 20 MiB |

## Plugin authority

`store.CheckPluginRequest` maps the command to `external_browser.read` for a
read action and `external_browser.control` for every other action
(`protocol.ExternalBrowserAction.ReadOnly`). The package must declare that
action, must be enabled as installed now, and the call must name the session
and worktree of its scope. A plan-mode turn may use only read actions. The
decision is repeated before and after the browser starts.

Read actions are `extract_text`, `extract_attribute`, `extract_links`,
`page_info`, `page_source`, the three frame switches, `navigate`, `back`,
`forward`, `refresh`, the four waits, `list_windows`, `switch_window` and
`downloads`.

## Tools

`Node.externalBrowserTools` binds each package tool whose handler operation is
`external_browser.read` or `external_browser.control`; the package's host
script prepares the call and formats the result around it
(`pluginScriptTurn.bindNative`). A tool call is `{action, arguments}`; an action
that does not match the declared operation is `permission_denied` "browser
action does not match the declared operation". Control tools need approval
through `store.ConfigureWorkspaceApproval`, which refuses them in plan mode.
The tool runs on the turn's context, so stopping the turn interrupts it.

Captures reach the model and the conversation through the existing tool-image
presentation (`inline_data` of an `external_browser` result).

## Files

Every path is worktree-relative and goes through `core.OpenFiles` with
`core.RestrictedFiles`.

- `file_upload` reads the file through the confined service, copies it into the
  session's temporary directory and hands that copy to the page.
- Page downloads land in the session's `downloads/` directory. `downloads` lists
  completed files; `save_download {file_name}` copies one into
  `assets/generated/` and never replaces a file.
- `screenshot` returns the image inline and writes it under `assets/generated/`
  only with `save_to_artifacts`. `print_to_pdf` always writes the PDF there.
  Saved files are named `browser-<request id>` and reported to the worktree
  watcher.

## Operations

All 49 actions are implemented. `operations.go` maps 46 page actions to
chromedp; `close_session`, `downloads` and `save_download` are session and file
operations in `browsers.go` and `files.go`.

| Group | Actions | Driver calls |
| --- | --- | --- |
| Navigation | `navigate`, `back`, `forward`, `refresh` | `Navigate`, `NavigateBack`, `NavigateForward`, `Reload` |
| Pointer | `click`, `double_click`, `right_click`, `hover`, `drag_and_drop` | `MouseClickNode`, `MouseEvent`, `DragAndDrop` |
| Keyboard and forms | `type`, `clear`, `focus`, `press_key`, `select`, `file_upload` | `KeyEvent`, `KeyEventNode`, `CallFunctionOn`, `dom.SetFileInputFiles` |
| Reading | `extract_text`, `extract_attribute`, `extract_links`, `element_state`, `page_info`, `page_source` | `Nodes`, `CallFunctionOn`, `Evaluate`, `Location`, `Title` |
| Script | `evaluate_js`, `scroll` | `Evaluate`, `CallFunctionOn` with promise await |
| Waiting | `wait`, `wait_for_element`, `wait_for_page_load`, `wait_for_text` | `Nodes`, `PollFunction` |
| Capture | `screenshot`, `print_to_pdf` | `CaptureScreenshot`, `ScreenshotNodes`, `PrintToPDF` |
| Dialogs | `handle_alert` | `page.HandleJavaScriptDialog` |
| Cookies | `get_cookies`, `get_cookie`, `add_cookie`, `delete_cookie`, `delete_all_cookies` | `network.GetCookies`, `SetCookie`, `DeleteCookies` |
| Windows | `list_windows`, `new_tab`, `new_window`, `switch_window`, `close_window`, `maximize_window`, `minimize_window`, `set_window_size` | `Targets`, `NewContext`, `browser.SetWindowBounds` |
| Frames | `switch_to_frame`, `switch_to_parent_frame`, `switch_to_default_content` | `FromNode` for same-site frames, `WithTargetID` for cross-site frames |
| Session and files | `close_session`, `downloads`, `save_download` | `browser.SetDownloadBehavior` and the confined file service |

## Desktop

The Desktop has no screen of its own for this feature. The package is bundled
disabled; the user enables it on the existing plugins page, and control tools
ask through the existing approval prompt.

## Tests

- `externalbrowser.TestCallsWithoutBrowser`: argument and result bounds, calls
  that need no browser, recovery of stale files, and a host without an
  installed browser: one `not_configured` fault for every call and an unchanged
  profile.
- `node.TestExternalBrowserRouting`: the public command's checks and admission,
  plugin authority, all 49 tools offered in a real turn, approval and plan
  mode. No browser starts; it skips without an installed browser.
- `//go:build integration` `externalbrowser.TestManagedBrowserIntegration` and
  `node.TestExternalBrowserNodeIntegration` launch a local Chrome or Chromium
  against loopback pages and skip without one. They cover every operation
  group, upload and download confinement, interruption, the session limit,
  session removal, a stopped turn, Node stop and crash recovery, and assert that
  no browser process survives.

## Deviations

| Deviation from Rust | Reason |
| --- | --- |
| chromedp over the DevTools pipe replaces thirtyfour with a chromedriver process | The feature must use an existing Go browser-automation library; Desktop and Host stay pure Go |
| Only an installed Chrome, Chromium or Edge is used; nothing is downloaded and a missing browser is `not_configured` | User decision of November 11, 2026. Rust downloaded Chrome for Testing and chromedriver into `tools/chromium` |
| The tools stay offered without an installed browser | The fault that names what to install must reach the model and the user |
| `new_tab` and `new_window` switch to the new window and load the URL there | The package describes the tools that way; Rust loaded the URL in the current window |
| `double_click`, `right_click` and `hover` send real input events | The DevTools protocol dispatches them directly |
| `select` sets the option by script and dispatches `input` and `change` | The DevTools protocol has no select command |
| A dialog is answered when it opens by the session's policy (dismiss by default); `handle_alert` sets the policy and reports whether a dialog opened since it last changed | An open dialog blocks every DevTools call on the page |
| A cross-site frame is a separate target; a frame that went away is reported as "the selected frame is gone; switched to default content" | Site isolation stays on |
| Closing the last window leaves the session without a window ("no browser window is open; open a new tab") | The browser process stays owned until the session closes |
| Captures are bounded by `files.PrepareImage` (1568 px side, 3 MiB) | Reuses the existing image bound instead of a second reducer |
| A removed conversation closes its browser | Rust kept it until Node stopped |
