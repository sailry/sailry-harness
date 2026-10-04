# Task Notes

An installable Sailry plugin that uses host operations without MCP. Zip this directory's contents with `plugin.json` at the archive root, install the archive through the plugin manager, and select it for a conversation. Each execution Node owns its installation and notes. Tools work independently of the optional desktop controls, including from Mobile.

The `dev.sailry.platform` extension declares tools with an `operation`, an optional model-facing `description`, and a `presentation`. Sailry assigns each tool a stable package namespace. Operation parameters are supplied by the host; packages cannot replace their validation, scope or permission checks.

| Operation | Parameters | Required action |
| --- | --- | --- |
| `progress.update` | `title`, `steps` with `description` and `state` | None |
| `storage.get` | `key` | `storage.read` |
| `storage.list` | `prefix`, `after`, `limit` | `storage.read` |
| `storage.set` | `key`, `value`, `expected_revision` | `storage.write` |
| `storage.delete` | `key`, `expected_revision` | `storage.write` |
| `settings.read` | None | None |
| `http.request` | `method`, `url`, optional `headers`, `body`, `credential`, `timeout_ms` | `http.request` |

Progress states are `pending`, `in_progress`, `completed`, and `skipped`. A progress tool declares `presentation: "progress"` and uses the shared conversation renderer, including on Mobile. Its result remains readable after the plugin is removed. Other tools default to inspectable result details.

Storage is private to the package on its Node. Revisions come from the previous read; zero creates a new key. Writes follow the turn's ordinary tool approval policy and are unavailable in planning mode. Admitted turns retain their package and settings revisions when the live plugin inventory changes. The host never retries an uncertain mutation under a new request ID.

The extension's `ui` contributions share `desktop.ui_entry`. The example registers a toggle, select, menu, refresh button, searchable note picker and statistic. Sailry renders these with native Kit controls and moves them into the existing compact menus as the conversation narrows. The entry publishes values through `publishContributions` and receives named interactions through `nextContributionEvent`; it does not render another composer or statistics panel. `reply_to` links an asynchronous result to its interaction sequence so an older search cannot replace newer results.

The script uses the captured Node's KV through `sailry/sdk`, including when the Node is remote. A refresh retries any uncertain write with its original request ID before reading preferences. Updating, disabling or leaving a plugin's context releases its UI instance. See `apps/desktop/src/plugins/host/sdk/api.d.ts` for public types. Builtin contributions use the same declarations with registered native handlers.

For an HTTP capability, declare the `http.request` action and optionally a tool using that operation. `requestHttp` prepares a durable Node request for `completeRequest`; the default timeout is 30 seconds (maximum 60), UTF-8 bodies are limited to 1 MiB, and response headers retain their separate values. Redirects and automatic retries are disabled. An optional `credential` names a secret settings field declared as `"x-sailry-secret": {"origin":"https://api.example.com", "header":"Authorization", "prefix":"Bearer "}`. The Node injects that protected value only for the declared origin. A lost response remains uncertain and must not be retried with a new ID automatically.

The resource-panel entry embeds the native `Conversation` component. It keeps the captured session and execution Node, with separate draft/focus state; closing the panel does not cancel Node tasks. The `ui_entry` continues to supply controls and statistics through the same public SDK.
