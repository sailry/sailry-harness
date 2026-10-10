# Sailry plugin API v1

Desktop plugins import the host-owned `sailry/sdk` module. It is registered in Rust for each mounted plugin and uses the existing Client → Node commands on local and remote connections. Do not bundle a private copy of the SDK, model response parser, or request recovery loop.

The desktop TypeScript contract is [api.d.ts](../apps/desktop/src/plugins/host/sdk/api.d.ts). The declaration is registered with GPUI Kit and checked against exported methods. Headless callbacks have a separate [Node SDK subset](../crates/node-runtime/src/plugins/script/api.d.ts); desktop-only exports are not available there.

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

Model prompts and response parsing belong to the package. `resolveModel` validates the selected model and reasoning effort against the Node catalog; credentials remain on that Node.

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

The pinned script Button does not expose composed accessibility names. `ComposedButton.new(id, { label, tooltip, size })` from `sailry/ui` reuses Kit's native ghost button for image targets. Optional `size` sets a square in pixels; activation arrives through `nextCardEvent()`.

Navigation and resource entries use `desktop.navigation` and `desktop.panel`. Scripted surfaces declare their JavaScript entry and resources. The host resolves entries from the captured Node inventory and immutable package; packages do not gain native privileges by choosing a familiar name. Standard conversation controls remain host-owned.

Workspace pages declare `desktop.navigation_options.retention` as `{ "mode": "none" }` (default), `{ "mode": "always" }`, or `{ "mode": "idle", "seconds": 300 }`. Idle seconds must be a positive integer. The host releases `none` views on navigation, retains `always` views until window close, and starts idle expiry when a page is hidden. Returning cancels expiry and runs the normal entry refresh. Package/settings changes, removal, and disconnected scopes still invalidate views. Plugins do not implement their own retention timers; releasing a view cancels its UI runtime, not already admitted Node tasks.

Settings ignore workspace retention: entering settings can preload bounded immutable resources without running scripts or reading business data. Opening a settings page creates its view and reads current data; switching releases that view, and leaving the settings area releases all preloaded files.

`settings_page` contributes settings navigation, placement and an optional shared enable control. A script page supplies `entry`; a schema-generated page omits it. Feature pages and workflows belong in the package, including bundled language-service and search settings. The host provides Kit composition and scoped service APIs, never package-specific native page selectors.

`code.settings.read` allows reading Node-wide language settings, catalog and registry metadata. `code.settings.write` allows saving settings, updating rules and cancelling installation. `code.manage` allows exact-descriptor installation/removal; `code.read` observes installations. `web.settings.read` and `web.settings.write` allow search settings and driver checks. These settings operations require the captured settings surface, without a worktree, session, turn or callback invocation. Agent lifecycle tools still require their original approval. Disabled packages can configure their own settings surface.

`nextCodeInstallations()` observes installation updates on the captured Node and closes with the view. Use `sailry/search.prepareSettings` with opaque `sailry/credentials` field handles to prepare credential updates; plaintext credentials never enter JavaScript. Complete and recover these requests through the ordinary SDK request API.

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

Tools can bind public operations (`progress.update`, `storage.get/list/set/delete`, `settings.read`, `http.request`) without an MCP server. Declare each operation's required action. Node owns approval, planning restrictions, cancellation and durable outcomes; a desktop JavaScript callback is not an Agent tool implementation.

The delegation package binds `agents.delegate`, `agents.guide` and `agents.wait` with the `agents.delegate` action. Delegation is synchronous unless `background: true` is requested. Background admission returns the child session and turn; guidance takes that session and a message, and waiting observes its canonical outcome. Guidance and waiting are native tools, not callback flow steps. Only the running main session can control its children. Children keep their captured permissions and configuration, and approval remains required where that capture requires it. Main guidance can interrupt a wait at the settled tool-batch boundary without cancelling background children; explicit Stop cancels them.

`requestHttp` runs on the captured Node and returns a durable request ID. It accepts UTF-8 text, a maximum 1 MiB body and a 1–60000 ms timeout (default 30000). Response headers preserve repeated values. Redirects and retries are disabled. A named credential comes from a secret schema field with `origin`, `header` and optional `prefix`; only Node injects it into that origin. An interrupted or oversized response reports an unknown outcome, and retrying the original ID never resends the request.

## Tool message content

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

Plugin commands retain the existing declaration, exact installed-package, Node, worktree and session checks. File/Git/conversation operations use the existing scoped protocol through `prepareRequest`; HTTP uses the declared Node service; the SDK grants no raw process or OS access. UI and game rules stay in plugins. Persistence, credentials, model execution and durable receipts stay in Rust.

The current development schema remains v1. Startup fills missing schema objects and storage fields with their defaults, validates the update on an isolated snapshot, saves a private backup and applies it transactionally. Invalid updates preserve the existing profile; startup never resets or deletes user data.

`credential_source: "github_cli"` opts a schema form into the protected GitHub CLI account source. The execution Node verifies the token field, Bearer header and official GitHub MCP endpoint before linking; changing the package name grants no access. The shared native credential form keeps account secrets outside plugin JavaScript.
