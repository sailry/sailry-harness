# Sailry plugin API v1

Desktop plugins import the host-owned `sailry/sdk` module. It is registered in Rust for each mounted plugin and uses the existing Client → Node commands on local and remote connections. Do not bundle a private copy of the SDK, model response parser, or request recovery loop.

The desktop TypeScript contract is [api.d.ts](../apps/desktop/src/plugins/host/sdk/api.d.ts). The declaration is registered with GPUI Kit and checked against exported methods. Headless callbacks have a separate [Node SDK subset](../crates/node-runtime/src/plugins/script/api.d.ts); desktop-only exports are not available there.

| Capability | Public methods |
| --- | --- |
| Public configuration and scope | `readSettings()`, `context()` |
| Model catalog and selection | `listModels()`, `resolveModel(settings, catalog, field)` |
| Model decision | `prepareModel(player, turn)`, `modelChoice(result, player, choices)` |
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

`modelCommand` is available when a plugin needs to inspect a prepared model command. Most plugins should use `prepareModel` directly. The `turn` object contains `instructions`, JSON `state`, and legal `choices` whose integer `id` equals its array index. Provider and decision models use the same selection API. Credentials remain on the execution Node.

```js
import { readSettings, listModels, resolveModel, prepareModel,
  completeRequest, modelChoice, forgetRequest } from "sailry/sdk";

const player = resolveModel(await readSettings(), await listModels(), "ai_model");
const id = prepareModel(player, turn);
const result = await completeRequest(id);
forgetRequest(id);
const { move } = modelChoice(result, player, moves);
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

Navigation and resource entries use `desktop.navigation` and `desktop.panel`. Scripted surfaces declare their JavaScript entry and resources. The host resolves entries from the captured Node inventory and immutable package; packages do not gain native privileges by choosing a familiar name. Standard conversation controls remain host-owned.

## Conversation component

`Conversation.new("chat")` embeds a resource panel's captured session and Node with independent focus and draft state. Declare both `conversation.read` and `conversation.control`. The native message list, composer, approvals, questions, model selection and history are shared with database and SSH assistants.

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

Conversation components are available in workspace and resource panels, including standalone navigation. Settings and composer workers show an unavailable state. Closing or disabling a panel releases subscriptions without stopping admitted Node work. The [task-notes example](../examples/plugins/task-notes) demonstrates captured sessions and a standalone assistant.

## Contributions and operations

Declare `ui` entries in `extensions.dev.sailry.platform`: a slot (`composer`, `context`, `statistics`), kind, localized label, icon, order and named handler. An optional `desktop.ui_entry` runs the state/event module. Publish state through `publishContributions` and consume actions through `nextContributionEvent`; responses include the event sequence in `reply_to`. Actions stay disabled until their matching reply, while searches can supersede earlier searches. The host renders normal and compact controls from the same declarations. Controls with `overflow: "menu"` appear in composer settings, including context controls; embedded conversations also place their context controls there. Metric details join the shared statistics popover.

Tools can bind public operations (`progress.update`, `storage.get/list/set/delete`, `settings.read`, `http.request`) without an MCP server. Declare each operation's required action. Node owns approval, planning restrictions, cancellation and durable outcomes; a desktop JavaScript callback is not an Agent tool implementation.

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

The runnable [tool-content MCP example](../examples/plugins/tool-content) returns a table, diff and image without a desktop entry.

## Boundaries

Plugin commands retain the existing declaration, exact installed-package, Node, worktree and session checks. File/Git/conversation operations use the existing scoped protocol through `prepareRequest`; HTTP uses the declared Node service; the SDK grants no raw process or OS access. UI and game rules stay in plugins. Persistence, credentials, model execution and durable receipts stay in Rust.

The current development schema remains v1. An incompatible existing profile is reported without conversion or deletion; tests use fresh isolated profiles.
