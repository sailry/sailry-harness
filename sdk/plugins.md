# Sailry plugin API v1

Desktop plugins import the host-owned `sailry/sdk` module. It is registered in Rust for each mounted plugin and uses the existing Client → Node commands on local and remote connections. Do not bundle a private copy of the SDK, model response parser, or request recovery loop.

The desktop TypeScript contract is [api.d.ts](../apps/desktop/src/plugins/host/sdk/api.d.ts). The declaration is registered with GPUI Kit and checked against exported methods. Headless callbacks have a separate [Node SDK subset](../crates/node-runtime/src/plugins/script/api.d.ts); desktop-only exports are not available there.
Their [model](../crates/node-runtime/src/plugins/script/models.d.ts) and
[installation](../crates/node-runtime/src/plugins/script/installations.d.ts) modules
share the scoped request contract with bounded snapshot observations.

| Capability | Public methods |
| --- | --- |
| Public configuration and scope | `readSettings()`, `context()` |
| Model catalog and selection | `listModels()`, `resolveModel(settings, catalog, field)` |
| Model request | `prepareRequest(command)`, `completeRequest(id)` |
| Durable requests | `prepareRequest(command)`, `completeRequest(id)`, `forgetRequest(id)` |
| Atomic Node mutations | `prepareTransaction(operations)`, `prepareNotification(content)` |
| Coalesced invalidation | `nextChange(cursor?)` |
| Model error handling | `errorCode(error.message)` |
| Plugin storage | `getValue(key)`, `listKeys(prefix?, after?, limit?)`, `setValue(key, value, revision)`, `deleteValue(key, revision)` |
| Captured session | `readSession()`, `readConversation(before?, limit?)`, `nextConversation(cursor?)`, `sendMessage(input, revision)`, `stopTurn(turn)`, `setSessionConfig(config, revision)` |
| Node HTTP | `requestHttp(request)` |
| Captured Git | `inspectGit()`, `prepareGitIndex(draft)`, `prepareGitCommit(draft)` |
| Declarative UI | `publishContributions(states)`, `nextContributionEvent()` |
| Low-level context, resources and UI | Existing `sailry` exports: `context`, `theme`, `Header`, `Image`, `Conversation`, `header_action`, `next_change` |

`Image.new(id, {path, fit, width, height})` renders declared package images through GPUI Kit.
`fit` defaults to `contain`; `cover` crops to the component bounds and `fill`
maps the image to those bounds. The public adapter exposes Kit's existing
`ObjectFit`, which the pinned script image export does not expose. Transparent
atlas composition can therefore share exact layer coordinates without changing
image alpha or implementing another renderer.
Optional positive pixel `width` and `height` set explicit image bounds and their
layout aspect ratio. Atlas layers can use these to avoid Kit inferring an
intrinsic source ratio from percentage heights during layout.

Model prompts and response parsing belong to the package. `resolveModel` validates the selected model and reasoning effort against the Node catalog; credentials remain on that Node.

Use `sailry/models` for public `search`, `versions` and `resolve` with `models.read`.
`register({target, definition})` and `prepare(model)` require `models.install` and
return ordinary prepared request IDs. Complete registration first, then prepare
installation with its returned model identity. Definitions retain pinned asset
digests; OCR and embedding target `node`, while speech targets `controller`.
Headless callbacks cannot install controller-local speech models.

`sailry/installations.read(ids)` observes only owned model or language installation
requests in their original captured scope. Language observations require `code.read`
or `code.manage`; model observations require `models.install`. Both retain the exact
prepared request IDs and expose only their resource category, never another plugin's
or the controller's installation feed. `watch(ids)` starts a Desktop subscription and `next()` consumes its
Client-reduced snapshots. Headless callbacks use bounded model-only `read(ids)` snapshots;
`watch` and `next` cannot retain live subscriptions beyond their callback budget.
SDK rows use
decimal-string revisions and explicit `target`. `dismiss(id)` removes a failed or
uncertain observation without cancelling work or changing the durable outcome.
Package installation and update are not exposed by these model APIs.

```js
import {readSettings, listModels, resolveModel, prepareRequest,
  completeRequest, forgetRequest} from "sailry/sdk";
const model = resolveModel(await readSettings(), await listModels(), "ai_model");
const id = prepareRequest({kind: "generate_plugin_text", data: {
  model: model.model, effort: model.effort, prompt: "Your package-owned prompt"
}});
const result = await completeRequest(id);
// Validate the response and keep uncertain requests for an explicit retry.
if (!result.Err || result.Err.code !== "outcome_unknown") forgetRequest(id);
```

Use `errorCode(error.message)` for model and configuration failures. It accounts for the native bridge's diagnostic prefix and returns a stable code for plugin localization.

Keep `id` while a request is unresolved. `completeRequest` observes the original durable receipt after a missing response; an unresolved receipt rejects with `unconfirmed`. A retry uses that same ID. A completed business fault is returned as `{Err: {code, message}}`; success is `{Ok: {kind, data}}`. Forget only confirmed requests, or drafts intentionally abandoned when a view or game closes. Closing a view does not cancel work already admitted by the Node.

Header actions accept an optional `disabled` boolean; the host applies it to the native Kit button.
An optional `menu` array uses the existing Menu item fields: `id`, `label`,
`icon`, `enabled`, `checked`, `separator`, `children` and `value`. The action
opens a Kit dropdown instead of emitting its trigger ID. Selecting an enabled
leaf emits that leaf's ID through `header_action()`; Header selections do not
emit Menu events or values. Without `menu`, flat actions remain unchanged.
Menus reuse the existing declaration validation and 256 top-level item limit.

## Storage

Declare `storage.read` and/or `storage.write` in `extensions.dev.sailry.platform.actions`. The Node derives the namespace from the authenticated plugin context; the caller cannot choose another plugin's namespace. Data belongs to the execution Node and plugin name, surviving package updates, configuration edits, removal and reinstallation. No localStorage, filesystem path or credential access is provided.

```js
import { getValue, setValue, completeRequest, forgetRequest } from "sailry/sdk";

const entry = await getValue("preferences");
const id = setValue("preferences", { sound: false }, entry.revision);
const result = await completeRequest(id);
forgetRequest(id);
if (result.Err) {
  // Keep the draft and report the fault; do not overwrite a revision conflict.
} else {
  const saved = result.Ok.data;
}
```

Revisions are decimal strings in the SDK to preserve integer precision. Missing keys initially have revision `"0"`; deleted keys retain a revision so stale writes cannot recreate them silently. `present` distinguishes stored JSON null from an absent value. Set and delete are compare-and-set operations and return a prepared request ID, not a success claim.

Keys are nonempty UTF-8 strings up to 128 bytes, without NUL. Values are JSON up to 256 KiB; each namespace permits 256 live keys and 8 MiB of live values. Listing accepts literal prefixes and returns at most 100 keys with an optional `after` cursor. Deleted keys are excluded from listing. Storage is ordinary application data, not a secret store.

## UI and animation

Use GPUI Kit controls and its native `transition` API for interpolated position, size and opacity. Plugin scripts sequence game events with `cx.sleep()` and `cx.notify()`; they do not need a per-frame rendering loop. Keep game rules and animation sequencing in the plugin.

`Loading.new(id)` from `sailry/ui` is the region loading overlay. It covers its
parent, blocks input to the content below, and shows a centered spinner. The
parent must be `relative()` and have real size (`flex_1`, `size_full` or a
fixed height). The overlay fades in only after 300ms, so a read that finishes
sooner shows nothing; it appears immediately when the system reduces motion.
Keep the page heading, toolbar and scroll container mounted while the first
read is pending. Show the overlay only for a region with nothing to display yet.
When a region already has content, keep showing it during refresh without the
overlay. Show an empty state only after a successful read.

`CardList` handles these states automatically. Its children are the row-content
slot: existing children always remain visible; without children, `loaded: false`
renders a sized `Loading` region and a successful read shows `empty_label`. Omit
the state props to use the original layout-only composition. The plugin still
owns requests, successful-read state, drafts and error reporting.

```js
import { CardList } from "sailry/ui";
const content = CardList.new("records", {
  loaded,
  empty_icon: "inbox",
  empty_label: text.empty,
}).children(records.map(renderRecord));
```

The pinned script Button does not expose composed accessibility names. `ComposedButton.new(id, { label, tooltip, size })` from `sailry/ui` reuses Kit's native ghost button for image targets. Optional `size` sets a square in pixels; activation arrives through `nextCardEvent()`.

Navigation and resource entries use `desktop.navigation` and `desktop.panel`. Scripted surfaces declare their JavaScript entry and resources. The host resolves entries from the captured Node inventory and immutable package; packages do not gain native privileges by choosing a familiar name. Standard conversation controls remain host-owned.

Workspace and scripted settings pages use Kit's retained View entities. The host loads each scoped page once and switches presentation to its existing `ScriptView` and `ShellRoot`; ordinary navigation does not create another VM or View. Returning runs the normal entry refresh without discarding the mounted page or its drafts. Hidden pages pause visibility-bound work. Package/settings changes, removal, invalid scopes and Node changes still invalidate their runtime; releasing a view does not cancel already admitted Node tasks. There is no separate KeepAlive or Snapshot cache and no package retention timer.

Use `Workspace.new` from `sailry/ui` to supply main content and an optional details child to the Shell's common right sidebar. `has_details` controls availability; the Shell owns its header alignment, divider, width and visibility toggle. Call `showWorkspaceDetails()` after an explicit selection to reveal a previously hidden sidebar without changing its resized width. Do not call it on every render.

Entering settings can preload bounded immutable resources without running scripts or reading business data. Opening a page creates its View on first use and reads current data; later section switches reuse that entity. Leaving the settings area releases preloaded files, while mounted page owners remain with the captured Node until invalidation or window close.

`settings_page` contributes settings navigation, placement and an optional shared enable control. A script page supplies `entry`; a schema-generated page omits it. Feature pages and workflows belong in the package, including bundled language-service and search settings. The host provides Kit composition and scoped service APIs, never package-specific native page selectors.

`code.settings.read` allows reading Node-wide language settings, catalog and registry metadata. `code.settings.write` allows saving settings, updating rules and cancelling installation. `code.manage` allows exact-descriptor installation/removal and explicit worktree-scoped server start/stop; `code.read` observes installations and running instances. `web.settings.read` and `web.settings.write` allow search settings and driver checks. Settings operations require the captured settings surface, without a worktree, session, turn or callback invocation. Agent lifecycle tools still require their original approval and cannot change service enable settings. Disabled packages can configure their own settings surface.

`nextCodeInstallations()` observes managed receipts and inventory on the captured Node and closes with the view. Install stages use the shared `sailry/installations` queue; a ready receipt is not the command's durable completion. Registered package availability, managed installation and running worktree instances are separate observations. Removal is serialized with installation for the same package and returns Busy before stopping or deleting a package currently in use. Use `sailry/search.prepareSettings` with opaque `sailry/credentials` field handles to prepare credential updates; plaintext credentials never enter JavaScript. Complete and recover these requests through the ordinary SDK request API.

A package can declare `enabled_service: "language"` with Host scope and `code.settings.write` to bind its switch to the Node language service. Only one installed package may own a service binding. Omitting it creates no service coupling. The host validates and applies the lifecycle atomically; the field grants no actions.

Game prompts, legal-move selection and response validation belong in each game package. Use `prepareRequest` with `generate_plugin_text`, then `completeRequest` and `forgetRequest`; the core model API does not construct game prompts or interpret moves.

The browser package owns annotation review and note drafts. `readBrowser()` and `nextBrowserChange()` expose the current `{token, element}` capture and capture error; `BrowserAnnotationImage` displays its protected screenshot without exposing image bytes to JavaScript. Compose the review with Kit controls, then call `submitBrowserAnnotation(token, reference)` with the JSON reference through `sailry/ui`, or cancel with `browserAction({kind: "cancel_annotation", token})`. The shared service verifies the captured element, token and note bounds before transferring the image and text to the original composer's draft; navigation or view changes invalidate that capture. `BrowserSurface.new(id, {covered: true})` hides the OS child view behind package overlays without unmounting or cancelling capture; coverage defaults to false. Surface and preview components bind the mounted view lease, so older panels cannot change coverage after remounting.

Knowledge retrieval is a shared Node service rather than a bundled feature plugin. Notes, reminders and other feature packages organize their own collections and expose their own settings and conversation tools using the storage and knowledge operations in the v1 contract. The built-in knowledge settings configure vector models only.

`ResourceTree.new(id, { items: JSON.stringify(items), selected, current })` takes a serialized `ResourceTreeItem[]` as its `items` field. The pinned Kit host bridge limits structured arguments to 16 nested levels, so the native host decodes and validates this recursive model while retaining Kit's tree state, events and native menus.

Optional `guides: true` adds dashed hierarchy guides to resource rows, following `indent` and `skip_depth`; it defaults to false and leaves tree interactions and the branch variant unchanged.

## Conversation component

`Conversation.new("chat")` embeds a resource panel's captured session and Node with independent focus and draft state. Declare both `conversation.read` and `conversation.control`. The native message list, composer, approvals, questions, model selection and history are shared with database and SSH assistants.

`Conversation.new("detail", { session: id, active: open })` embeds an existing, non-archived session from the panel's captured Node. It resolves the session's project and worktree from the canonical snapshot and uses the same native controls. `session` cannot be combined with `assistant` or `resource`. Set `active: false` to release the mounted conversation when a dialog closes; this does not stop admitted work. Reactivating the component can select another session. Changing session props while active cannot retarget its mounted chat.

A plugin can also declare assistants under `desktop.conversations` and mount one from its workspace or resource panel:

```json
{
  "conversations": [{
    "id": "notes",
    "resource": "plugin",
    "context": "Help the user organize their notes",
    "tools": [
      { "kind": "plugin", "name": "read_note" },
      { "kind": "plugin", "name": "save_note" }
    ]
  }]
}
```

```js
import { View, div } from "gpui-kit";
import { Conversation } from "sailry";
export default class Assistant extends View {
  render() {
    return div().size_full().child(Conversation.new("notes", { assistant: "notes" }));
  }
}
```

`resource` is `plugin`, `workspace`, `database` or `ssh`. A plugin assistant uses a private Node workspace; a workspace assistant uses the panel's captured project/worktree. Database and SSH assistants require a concrete resource in the same Node: `{ assistant: "database", resource: { kind: "database", id } }`. Their existing native resource restrictions still apply.

A `builtin` tool selector names a registered native tool. A `plugin` selector names this package's public operation tool, or its MCP tool when `server` is supplied. Node resolves aliases, intersects permissions and enabled features, and keeps core questions, session title and compaction operations available. Context and selectors come from the immutable installed package; JavaScript cannot replace them or request tools from another package.

Mounting does not create a session. The first send uses normal Node admission; restore matches the exact package reference, assistant, resource and workspace. Use a stable component ID for each resource. Changing props under an existing ID cannot retarget a mounted chat. Model changes use ordinary revision-checked session configuration; the assistant binding stays immutable. The native new/history controls keep sessions within that binding.

Conversation components are available in workspace and resource panels, including standalone navigation. Settings and composer workers show an unavailable state. Closing or disabling a panel releases subscriptions without stopping admitted Node work. The [task-notes example](../plugins/examples/task-notes) demonstrates captured sessions and a standalone assistant.

## Contributions and operations

Declare `ui` entries in `extensions.dev.sailry.platform`: a slot (`composer`, `context`, `statistics`), kind, localized label, icon, order and named handler. An optional `desktop.ui_entry` runs the state/event module. Publish state through `publishContributions` and consume actions through `nextContributionEvent`; responses include the event sequence in `reply_to`. Actions stay disabled until their matching reply, while searches can supersede earlier searches. The host renders normal and compact controls from the same declarations. Controls with `overflow: "menu"` appear in composer settings, including context controls; embedded conversations also place their context controls there. Metric details join the shared statistics popover.

Tools can bind public operations (`progress.update`, `storage.get/list/set/delete`, `settings.read`, `http.request`, `artifacts.write/read/list`) without an MCP server. Declare each operation's required action. Node owns approval, planning restrictions, cancellation and durable outcomes; a desktop JavaScript callback is not an Agent tool implementation.

Packages that submit automatic input with `ContinueTurn` can declare
`continuation_marker` in `extensions.dev.sailry.platform`, using a localized
`label` and optional `locales` map. Node captures the package reference and marker
in `conversation::Run.continuation`; controllers display this as a system marker
instead of a user message. The model still receives the submitted input. Without
a declaration, controllers use their generic continuation label. `Run.origin`
continues to identify inherited history independently of automatic input.

An explicit controller action can atomically resume an idle queue and admit a
fenced continuation with `prepareTransaction`. Put a `queue_pause` operation
before the state write and `continue`, with data `{session, expected_revision,
paused: false}`. The revision comes from `readConversation().page.queue.revision`
as an exact decimal string; queue operation receipts use the same representation.
This requires `conversation.control` and the captured session scope. Held input
or running work still rejects the continuation and rolls back all operations,
including the queue pause change. Automatic completion callbacks leave the queue
pause unchanged.

The delegation package binds `agents.delegate`, `agents.guide` and `agents.wait` with the `agents.delegate` action. Delegation is synchronous unless `background: true` is requested. Background admission returns the child session and turn; guidance takes that session and a message, and waiting observes its canonical outcome. Guidance and waiting are native tools, not callback flow steps. Only the running main session can control its children. Children keep their captured permissions and configuration, and approval remains required where that capture requires it. Main guidance can interrupt a wait at the settled tool-batch boundary without cancelling background children; explicit Stop cancels them.

`requestHttp` runs on the captured Node and returns a durable request ID. It accepts UTF-8 text, a maximum 1 MiB body and a 1–60000 ms timeout (default 30000). Response headers preserve repeated values. Redirects and retries are disabled. A named credential comes from a secret schema field with `origin`, `header` and optional `prefix`; only Node injects it into that origin. An interrupted or oversized response reports an unknown outcome, and retrying the original ID never resends the request.

## Session HTML documents

Declare `artifact_view` in `extensions.dev.sailry.platform` with a package-relative
`template`, ordered `styles` and `scripts` paths, and optional `libraries` containing
`mermaid`, `echarts` or `mathjax`. Declare permitted fragment CSS classes in
`allowed_classes`; undeclared classes are removed, and omitting the list permits
none. The list accepts up to 64 distinct names, each up to 64 ASCII characters,
starting with a letter and containing only letters, digits, `_` or `-`.
All resources must be declared in the package. Sanitization uses the immutable
package captured at turn admission, including its class declarations.
The Node freezes these assets with each revision. The built-in Brief package owns
its prompts, HTML examples, preview controls and export menu; the host dispatches
by the captured declaration rather than a package name.

Native tools bind `artifacts.write`, `artifacts.read` or `artifacts.list` and declare
`artifacts.write` or `artifacts.read` actions. Write accepts `title`, `summary` and
`html`; updates also require `id` and `expected_revision`. Fragments allow headings,
text, tables, bounded forms, diagram/math source and validated chart JSON, up to
512 KiB. Scripts, remote assets and executable attributes are removed. The receipt
contains a typed document card and sanitization warnings, never the renderer assets.
Only a running main session may write; Plan mode permits this exact native operation
into profile-owned document storage, without broader write permission.

The controller `sailry/artifacts` module is scoped to an opened document card.
`artifactResource()` returns its ID, captured conversation and an optional inherited
revision. `readArtifact(id, revision?)` reads
the latest or an exact immutable revision; `listArtifactRevisions(id, before?)`
returns bounded pages. `nextArtifactChange(cursor?)` observes coalesced canonical
changes and connection state. `ArtifactSurface.new(id, {artifact, revision?, labels?})`
mounts the frozen package renderer; omitting revision follows the latest, while an
explicit revision pins it. `nextArtifactEvent()` delivers editing events so package
controls can pin a draft too. Older revisions cannot submit forms. Removing a
package preserves its captured document and renderer assets; Desktop reports that
the plugin was uninstalled instead of mounting its controls. New turns no longer
receive that package's instructions or tools.

Inherited cards retain the original document owner. The captured destination
conversation can read only revisions with exact receipts in its visible history;
its revision list and latest revision use that same scope. Inherited surfaces pin
their published revision and cannot submit forms. Owner reads retain the document
inventory across conversation rewinds. Removing the original session preserves
documents while another live conversation retains their receipts, including in
profile backups, and removing the last reference releases them.

The renderer exposes `window.sailryDocument.mount`, `retheme`, `setWritable`,
`submitted` and `readonlyExport`. Form events carry the captured document, revision,
session and named fields; the native controller validates them and submits ordinary
conversation messages. Unknown outcomes retain the original request and answers.
`exportArtifact({artifact, revision, format: "html" | "png"})` captures the live view
when available, renders charts and formulas offline, removes scripts and forms,
adds branding, and uses the native save picker. Cancellation returns false; a
successful save opens the containing directory.

## Tool message content

`display.input.summary` can declare `result_path`, a JSON Pointer into the matched
native result. A nonblank string at that path replaces the displayed argument
after the result arrives, before applying the field's existing format. Missing,
blank or non-string values keep the original summary, including attachment IDs.
This display-only hint is captured with the call; it changes neither tool
arguments nor resource identity. Other input fields cannot declare it.

A tool declaration can set `presentation: "content"`. Return `sailry_content` in the native result or in MCP `structuredContent`; ADK preserves the latter under the result's `output` field. The declaration is captured by the execution Node with the tool call. Model arguments and output alone cannot select this presentation.

```json
{
  "sailry_content": {
    "version": 1,
    "blocks": [
      { "kind": "table", "columns": ["File", "State"], "rows": [["hello.txt", "Updated"]] },
      { "kind": "diff", "path": "hello.txt", "text": "@@ -1 +1 @@\n-before\n+after\n" },
      { "kind": "image", "index": 0 }
    ]
  }
}
```

The content is finite data. Tables contain literal strings with equal row and column widths. Diffs contain unified diff text and a path used only for syntax and display. Images refer to the original inline-data index of a `Image` in that same result; MCP servers send image bytes through normal MCP image content. Image URLs, arbitrary attachment IDs, executable markup and UI scripts are not accepted. Downloads retain the existing authenticated conversation boundary.

Content allows 1–16 blocks and at most 256 KiB of serialized JSON. Tables allow 1–64 columns, 1000 rows, 1024 bytes per column label and 16 KiB per cell. Diffs allow a 4096-byte path without control characters and 4096 lines. Tables and diffs accept `truncated: true` to disclose partial output. Unknown fields or kinds, unsupported versions, mismatched rows, missing or duplicate image references and failed results fall back to literal output. The shared Client validates the matched call and result, so controllers can consume the same content.

Desktop renders these blocks with the common Kit table, diff and authenticated image viewer. Native call state, approval, cancellation, folding and history order still belong to the conversation. Expand **Details** for the unchanged result. History remains readable after package removal and Node restart; no plugin script runs to render saved tool content. The built-in database query uses this same contract.

Plans retain `presentation: "progress"` and the `progress.update` operation. Questions and plan confirmation retain `ask_user` and the existing question state; external MCP servers can use form or URL elicitation. Content blocks cannot create approvals or questions or submit answers.

The runnable [tool-content MCP example](../plugins/examples/tool-content) returns a table, diff and image without a desktop entry.

## Boundaries

Headless `sailry/models.list({target:'node', kind:'ocr'})` returns available OCR
definitions and installation state under `models.read`. Package handlers use
the native `models.install` and `models.remove` operations for explicit OCR
lifecycle tools. Removal requires the original claimed command approval and
keeps definitions and settings. It shares the existing OCR installation lane
without producing a separate installer card. Plan turns cannot mutate models;
callbacks cannot directly dispatch OCR removal.

Headless `listRoles()` requires `roles.read` and returns public catalog profiles,
with decimal-string revisions and model identifiers, not session provider
snapshots or credentials. Package-owned handlers prepare `roles.save`,
`roles.remove` and `roles.apply` with `roles.write`; direct callback writes are
not exposed. Mutations consume the original canonical tool approval and its exact
prepared command through durable Node admission. Plan turns cannot mutate roles.
`roles.apply` accepts a complete bounded set of distinct `{id, revision}`
references and `expected_revision`, all revisions as decimal strings. Its session
comes only from the captured operation context. The turn initializer exposes
`session_revision` only when this operation is selected, without conversation
configuration. Application returns only the session ID, new revision and selected
references; existing sent turns and children remain immutable. Catalog changes
alone affect new sessions, not an existing roster.

Plugin commands retain the existing declaration, exact installed-package, Node, worktree and session checks. File/Git/conversation operations use the existing scoped protocol through `prepareRequest`; HTTP uses the declared Node service; the SDK grants no raw process or OS access. UI and game rules stay in plugins. Persistence, credentials, model execution and durable receipts stay in Rust.

The current development schema remains v1. Startup fills missing schema objects and storage fields with their defaults, validates the update on an isolated snapshot, saves a private backup and applies it transactionally. Invalid updates preserve the existing profile; startup never resets or deletes user data.

`credential_source: "github_cli"` opts a schema form into the protected GitHub CLI account source. The execution Node verifies the token field, Bearer header and official GitHub MCP endpoint before linking; changing the package name grants no access. The shared native credential form keeps account secrets outside plugin JavaScript.
## Inline conversation cards

Declare `cards` in the v1 extension. Each card has a unique `key`, a JavaScript
`entry` listed in `desktop.resources`, and a closed `props_schema`. Properties
are bounded strings, booleans, or integers, not nested data. Set
`additionalProperties: false`; strings require `maxLength` at most 4096.
Credentials, callback addresses, OAuth URLs, scripts and commands are not public
card properties. A card-only package does not need a workspace navigation entry.

An admitted host tool imports `card` and `status` from `sailry/cards`:

```javascript
const props = await status("owned-server");
return {sailry_content: {version: 1, blocks: [
  card("connection", props, {label: "Connect", locales: {"zh-CN": "连接"}})
]}};
```

`status` reads only the configured boolean of an HTTP MCP server owned by the
captured package. Headless tools cannot begin login. Node stamps the canonical
block with its package digest, Node, session, worktree, turn, stable call and
block position. Invalid drafts lose their presentation without changing the
original tool result. A failed result may keep its validated card and original
error; the card does not make that operation succeed.

The card entry extends Kit `View` and uses the narrow `sailry/card` module, not
the ordinary workspace SDK. It provides `state`, `theme`, `status`, `nextChange`,
`publishProps`, `toast`, native Kit `Action` buttons and native `Qr` images.
`publishProps` changes only this instance's display state, never the canonical
receipt. Package code owns layout, localization and refresh handling.
`nextChange` has a string cursor and an optional unique refresh action string.

Capabilities are independent and optional:

- `qr: {payload: "/field", expires_at: "/expiry"}` displays a public string
  property, bounded to 2048 UTF-8 bytes, with an optional epoch-millisecond
  expiry. Encoding stays in memory. Notion MCP login is not a device-QR flow.
- `refresh: true` permits an explicit native `Action` with `{kind: "refresh"}`.
  The package reads current status when it receives that action; no model turn
  or failed tool is automatically replayed.
- `browser` contains named targets with fixed HTTPS `origins`, bounded
  `modes` (`embedded` and/or `external`), and a `source`: either
  `{kind: "props", pointer: "/publicUrl"}` or `{kind: "mcp_authorization"}`.
  The latter requires the card's `mcp_server`; its URL remains native and private.

`Action.new(id, {label, action})` binds an actual Kit Button to a native user
gesture. Intents are `connect` (optionally a declared target and mode), `cancel`,
`refresh`, or `browser` with a declared target and mode. There is no callable
JavaScript connect/browser function. A mount may read status but cannot start
login. OAuth uses the existing Client observer and Node MCP authorization
engine, including original durable request IDs and outcome inspection. URLs,
PKCE, callbacks and tokens are never exposed to scripts, history or models.
Remote login still uses the controller's private loopback callback and the
ordinary remote command path, not a Node HTTP relay.

Embedded targets use an isolated, non-instrumented native Kit WebView in the
inspector. They are not Browser resources or Agent tools and have no DOM/IPC
bridge, download or popup permission. Only declared origins and the native
flow's exact loopback callback are navigable. MacOS uses a non-persistent data
store. Embedded isolation is not accepted on Windows or Linux; an independently
declared external action remains available. Opening an external URL is a launch
request, not proof that the browser or account flow succeeded.

Only a visible, connected, latest own main-session receipt can mount or act.
Historical turns, forks, child sessions, rewinds, disabled/replaced packages and
disconnection retain localized static fallback. Hidden mounts stop subscriptions
and clear private pending URLs. Every service action rechecks canonical history,
current package settings and scope at Node admission. A login may advance
settings while immutable captured code remains available under the same digest.
Uncertain side effects retain their original request and require outcome
inspection, never automatic replay. These contracts do not establish real
Notion account or embedded sign-in acceptance.

An optional `registration` declaration binds application creation to the existing
settings schema: `{provider:"feishu", client_id:"app_id",
client_secret:"app_secret", tenant_scopes:[...]}`. The ID must be a public string
field; the secret must use a protected Feishu application binding whose
`client_id` is that same field. Account OAuth and application registration are
distinct declarations and cannot coexist on one package.

`BeginPluginRegistration` is a native controller settings action, not a headless
callback or generic JavaScript command. It shares plugin login attempt IDs,
durable admission, caller-scoped observation and cancellation. Constructors
observe only; creation and refresh require an explicit native user action.
The execution Node polls the fixed provider endpoint with provider backoff and
no automatic restart. Verification URLs remain transient native controller
state; device codes, application secrets and unused provider identity fields do
not enter plugin JavaScript, model results or public snapshots.

Completion saves issued credentials through the existing revision-checked
settings transaction. Configuration changes during registration preserve the
newer settings and fail the old attempt. Native cleanup retains its original
attempt identity and can cancel it after a settings change or package removal.
An unknown result must be inspected using the original request identity, not
replayed as a new application creation. Configured fields do not establish
provider publication, document access or MCP entitlement.
