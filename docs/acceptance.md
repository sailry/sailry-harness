# Manual acceptance for 0.1.6

The checks a person runs in the real app before the release is tagged. Tests
and CI cover the rest ([CONTRIBUTING.md](../CONTRIBUTING.md#checks)). Run the
areas in order; each item names where to click and what must be seen.

- The user's acceptance profile is `~/.sailry-go`, the default of a
  development build started without `--data-dir`. Only the user runs against
  it. Development runs use `just desktop`, which is
  `--data-dir .runtime/profiles/desktop`. Nothing runs against `~/.sailry`.
- **[account]** needs a real provider account or a signed-in vendor CLI.
- **[macOS]**, **[Windows]**, **[Linux]** need that system. Unmarked items run
  on any Desktop system and are verified on macOS only so far.
- A failed item is reported with the area, the item's text and what was seen.

## 1. Start and Demo mode

- [ ] Start Desktop. The window opens on the welcome page or the last
      conversation; no start failure dialog appears
- [ ] Start a second Desktop on the same profile. The running window comes to
      the front and no second window stays
- [ ] Settings › Experimental › turn Demo mode on. The sidebar shows the sample
      projects and conversations at once, without a restart
- [ ] Look over the sidebar, the header, a sample conversation and the host
      page. Nothing marks the data as a demo, a preview or a sample
- [ ] Open several sample conversations. Each shows a transcript; none is empty
      or shows a loader that never ends
- [ ] In a sample conversation send a message, press Stop, press Retry, and
      queue two messages during a reply. The sample reply plays each time
- [ ] Open the Usage page from the rail after those sends. It shows the sample
      ledger. Turn Demo mode off and open it again: no request or usage figure
      was added by the sample sends
- [ ] Open the project menu, the panel's Files and Git tabs and the
      notification centre in Demo mode. Every item either works on sample data
      or is absent; none reports a failure
- [ ] Turn Demo mode off. The real conversations return and none of the sample
      messages sent above exists in them

## 2. Conversation and composer

These use a real conversation with any working model channel.

- [ ] Open a conversation longer than the window. The transcript fills the pane
      to the bottom edge, the composer floats over its end, and the turns fade
      out above the composer instead of being cut by a line
- [ ] Scroll to the last turn. Its last line is fully readable above the
      composer
- [ ] Split the workspace by dragging a session onto it. Each pane has its own
      docked composer and fade
- [ ] While a reply runs, send three messages. The queue tray appears over the
      composer under a header reading "3 queued messages"; clicking the header
      folds and unfolds the rows
- [ ] The queue rows show the messages only: no wait label and no row for a
      background result
- [ ] Click a web link in a reply. It opens in the conversation's right panel
- [ ] Click a file path in a reply. It opens as a tab of the right panel
- [ ] Click the same link with Command (macOS) or Control (elsewhere). It opens
      in the system browser
- [ ] Click Add attachment in the composer. The app's file picker opens, not
      the system's file dialog; choose two files and both appear as cards
- [ ] Drag the picker by its top band. It moves and stays inside the window
- [ ] Settings › Conversation › move the message text size slider. Transcript
      text follows a pixel at a time; the composer's figures and controls keep
      their size
- [ ] Ask for a Mermaid flowchart and a sequence diagram. Both are drawn in the
      reply, in the theme's colours

## 3. Model requests and the connection

- [ ] **[account]** Make a request fail in a retryable way: disconnect the
      network, or point a channel's base URL at an address that refuses the
      connection, then send a message. The turn shows "Retrying request 1/9"
- [ ] Keep it failing and time the rows. "Retrying request 2/9" appears 60
      seconds after 1/9, and each next one 60 seconds later
- [ ] Press Stop during a wait. The turn ends at once
- [ ] Restore the network during a wait. The next attempt answers and the
      reply continues in the same turn
- [ ] Use a wrong API key. The turn fails at once without a retry row
- [ ] With a remote Host connected, open one of its conversations and stop the
      Host or cut its network. After about 5 seconds the banner "Reconnecting"
      appears after the last turn, with no button
- [ ] Start the Host again. The banner goes away by itself and the
      conversation continues
- [ ] Open a conversation on a healthy connection, and switch between
      conversations quickly. No banner flashes

## 4. Provider sign-in

All in Settings › Models › Add provider.

- [ ] **[account]** ChatGPT: choose the ChatGPT sign-in, complete it in the
      browser. The channel is saved, "Update models" lists models and a message
      gets a reply
- [ ] ChatGPT: before signing in, open Advanced settings. The catalog version
      and User-Agent fields are filled with the values in use, not empty
- [ ] Claude and Copilot: open Advanced settings. Claude shows a filled
      User-Agent; Copilot shows a filled User-Agent, editor version and editor
      plugin version; neither shows the other's fields
- [ ] **[account]** Copilot: choose the sign-in named Copilot. A device code
      and the GitHub address appear; enter the code on GitHub. The channel is
      saved, "Update models" lists models and a message gets a reply
- [ ] **[account]** Copilot: leave the channel idle for an hour, then send a
      message. It replies without asking to sign in again
- [ ] WorkBuddy is listed under Settings › Plugins as installed on a new
      profile, without visiting the Market
- [ ] WorkBuddy: start a sign-in. A confirmation titled with the channel shows
      the warning that the account may be suspended, once. Cancel: no browser
      opens and nothing is saved
- [ ] WorkBuddy: start it again. The warning is asked again. With the app in
      zh-CN the warning is in Chinese
- [ ] **[account]** WorkBuddy: Continue and complete the sign-in with an
      account that may be lost. "Update models" lists only free models
- [ ] **[account]** WorkBuddy: in the package's settings turn "Free models
      only" off for that site and press "Update models". Charged models appear.
      Turn it on again and update: they are gone
- [ ] **[account]** WorkBuddy: on a profile with a WorkBuddy channel saved by
      an earlier build, send a message before updating. The request is refused
      with a message that says why and nothing is charged; after "Update
      models" a free model replies
- [ ] WorkBuddy: the package's settings show "Automatic check-in" off on a
      profile that never saved them
- [ ] **[account]** Antigravity: with `agy` installed and signed in in a
      terminal, install the Antigravity package from the Market. Its details
      name the permission "Run its declared programs on this device"
- [ ] **[account]** Antigravity: add its channel, press "Update models", choose
      a model and send a message. The reply streams; a tool call runs as
      Sailry's own tool with its usual row and approval
- [ ] Antigravity without `agy` on the search path, or signed out: the failure
      names the missing command, or says to run `agy` in a terminal and sign in
- [ ] Antigravity is not installed on a new profile until chosen in the Market
- [ ] Copilot and Claude: start a sign-in. A confirmation titled with the
      channel shows the warning about the provider's terms, once. Cancel: no
      device code or browser appears and nothing is saved. Start again: it is
      asked again
- [ ] Antigravity: in Add provider choose Antigravity. A confirmation titled
      "Add Antigravity" shows the warning before the form. Cancel: the editor
      stays on its step and no channel is created. Continue: the form opens
- [ ] Editing a saved Antigravity, Copilot or Claude channel asks nothing
- [ ] Cursor is not installed on a new profile until chosen in the Market
- [ ] **[account]** Cursor: install the official CLI, check `agent --version`
      and run `agent login` in a terminal. Install the Cursor package from the
      Market and choose Cursor in Add provider: the warning is asked before
      the form. "Update models" lists models; if it says the agent did not list
      its models, this CLI build does not report them and the item fails
- [ ] **[account]** Cursor: send a plain message. A reply arrives, and no
      failure says the agent does not offer the mode `ask`
- [ ] **[account]** Cursor: ask it to read a project file. The call appears as
      Sailry's own tool row with its usual approval, and its result reaches the
      next step
- [ ] **[account]** Cursor, the check that its own tools stay off: ask it to
      run `touch probe.txt` in its working directory, to read `/etc/hosts` by
      absolute path and to fetch a URL, each "with your own tools, not
      Sailry's". No `probe.txt` exists under the profile's
      `plugins/data/cursor/programs/agent`, and each action either does not
      happen or appears as Sailry's own tool with its approval. Do not publish
      the package until this item passes: the CLI documents no switch that
      turns its tools off, so this is the only proof
- [ ] **[account]** Cursor: choose another model and send. The new model
      answers. Stop a long reply: it stops at once
- [ ] **[account]** Cursor: run `agent logout`, then send. The failure says to
      run `agent login` in a terminal; no reply text carries the error

## 5. Pages

- [ ] Open a project's page. Its More menu opens; every item acts on the
      project. There is no trust item and no AI CLI item or button
- [ ] The project page lists the project's sessions and terminals as rows of
      the Settings list look. An archived session's menu has Unarchive, and it
      returns to the sidebar
- [ ] With the sidebar sorted by Created, drag a project's session onto
      another session of that project. A line shows the target place and the
      row moves there
- [ ] Open the host page. Its header names the system, the Host version and
      the Protocol version; the page has no Runtime section and no Projects
      section
- [ ] Move the pointer over a host chart. A tooltip shows the sample under the
      pointer
- [ ] The host page's process table has a filled header and compact rows, like
      the Usage tables; rows have no letter tile and no memory bar
- [ ] **[macOS]** A Mac with a GPU shows the GPU card with its load
- [ ] Right-click a process, choose End process on a process of your own. It
      ends and leaves the table
- [ ] Open the notification centre from the header. Notices are full-width rows
      of one card; each shows its title and, on a second line, its message; a
      row's actions sit beside its button
- [ ] The Usage page, opened from the rail, shows compact plain-text tables:
      no provider tile, no project tag, no cache bar
- [ ] Open the Market. A card shows the package's icon, name and description,
      and no source line
- [ ] Click the rail's Settings button. Settings opens; click it again and
      Settings closes
- [ ] Open the model picker in the composer. Groups fold; the chosen model is
      marked by a neutral fill, with no check mark and no blue
- [ ] Open a project's Git tab. History is a graph with lanes; Load more
      extends the same list; the diff footer switches Unified and Split
- [ ] Start a goal with `/goal <text>`. The badge shows an icon, the state and
      a time; its panel has Pause or Resume and End only

## 6. Appearance and language

Repeat for light and dark (Settings › Appearance) and for English and 简体中文
(Settings › General › Language). The language changes at once, with no
restart toast.

- [ ] Docked composer and fade: the fade matches the pane's background in both
      modes, with no band of another colour
- [ ] Queue tray: its fill is the composer's own; the header reads
      "3 queued messages" and "3 条消息排队"
- [ ] File picker: selection is the neutral fill in both modes; headings and
      places are translated
- [ ] Host page: header, chart tooltips and process table are readable in both
      modes; "Host version" and "Protocol version" are translated
- [ ] Notification centre, Usage tables and Market cards: separators are
      visible in both modes and no text is clipped in either language
- [ ] Retry row: "Retrying request 2/9" and "重试请求 2/9"
- [ ] Connection banner: "Reconnecting" and "正在重连"
- [ ] Sign-in dialogs: "Advanced settings", "Update models" ("更新模型") and
      the WorkBuddy warning are translated, and no label ends in a full stop
- [ ] Menus, popovers, dialogs and tooltips opened above are frosted in both
      modes; right-click menus are the system's own

## 7. Other systems

Only the user or CI can run these. A local build for another system proves
nothing about its behaviour.

- [ ] **[Windows]** Desktop starts from the packaged build, shows the window
      frost and the title bar buttons, and section 2 passes
- [ ] **[Windows]** Control-click opens a link in the system browser; a plain
      click opens the panel
- [ ] **[Windows]** The file picker lists the drives and the standard folders,
      and Add attachment attaches a file from another drive
- [ ] **[Windows]** Add host offers WSL instead of the agent; installing a
      Host into a distribution pairs it
- [ ] **[Windows]** The host page of a Windows host: the process menu has Force
      quit and no End process
- [ ] **[Windows]** Antigravity: the channel replies with `agy` installed.
      Known difference: its tool-blocking hook uses `printf` and is inert
      there, so confirm no agent tool of its own runs during a reply
- [ ] **[Windows]** Run `just test-integration ./internal/node/wsl
      ./internal/node/store`
- [ ] **[Linux]** The Host archive installs and starts, pairs with a Desktop,
      and a conversation on it replies
- [ ] **[Linux]** The host page of a Linux host shows real volumes without
      pseudo mounts, and no GPU card
- [ ] **[Linux]** The file picker browsing that Host lists its home and
      standard folders
- [ ] **[Linux]** Antigravity on a Linux Host with `agy` signed in there: the
      channel replies
- [ ] CI: the `integration` input of `ci.yml`, left empty, passes on every
      leg
- [ ] Mobile is accepted on a device separately ([mobile.md](mobile.md)); no
      item here covers it
