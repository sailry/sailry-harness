# Project summary

An installable example with a Skill, headless callbacks and a GPUI Kit desktop
panel. It reads the captured execution Node's Git status, previews a Markdown
report and saves `project-summary.md` only on an explicit save. File notifications
invalidate the preview; they do not automatically refresh or save it.

## Use

Register a project containing this package on the execution Node and install
`examples/plugins/project-summary` from its worktree-relative path in plugin settings.
Open a conversation in that Node's Git worktree, then select Project summary in
the resource panel. Skill use does not require an open panel.

The panel captures its Node, worktree, optional session and exact package
revision. Its strings live in `desktop/locales.js`. `include_untracked` initializes
the checkbox from public Node settings; changes remain a panel draft unless saved
through plugin settings. Refresh preserves the current checkbox choice.

An externally changed report requires refresh before saving. An uncertain save
retains its original request ID for recovery; it does not submit another write.
The report is a bounded file, not a Git commit or rollback. This package requests
Git reads, not permission to create commits.

## SDK and request lifecycle

Desktop uses the host-owned `sailry/sdk` module for public settings, `inspectGit()`
and durable requests. Contracts and request recovery rules are described once in
[the SDK guide](../../../plugins/SDK.md); the authoritative types are in the
[desktop declaration](../../../apps/desktop/src/plugins/host/sdk/api.d.ts).

Keep an unresolved request ID until its outcome is confirmed. A protocol fault
is different from an adapter exception. Revision conflicts require inspection;
an exception does not establish that a write never happened. Releasing a draft
or closing a panel does not cancel an operation already admitted by Node.

## Headless callbacks

`host/main.js` exports `summarize` and `save`. They run on the execution Node
without a desktop view and can be called through the generic dispatch queue.
Report policy and localization belong to this package; Git, files, settings
and durable admission remain Node services.

A captured desktop consumer can call the callback through the shared SDK:

```js
import { prepareRequest, completeRequest, forgetRequest } from "sailry/sdk";

const id = prepareRequest({
  kind: "call_plugin",
  data: { handler: "save", input: {
    locale: "en", include_untracked: true, expected_revision: null,
  } },
});
const result = await completeRequest(id);
// Handle result.Err before releasing a confirmed request.
forgetRequest(id);
```

`null` expects a new report; replacing an existing file requires its observed
revision. The callback returns a `plugin_result` containing the write outcome.
An interrupted unknown outcome needs inspection, not another callback request.
Multiple callback writes are not an atomic transaction.

The Node-owned QuickJS VM loads digest-verified declared resources and the
[headless SDK subset](../../../crates/node-runtime/src/plugins/script/api.d.ts).
It has no filesystem, process, fetch or Node.js globals. Desktop UI exports are
not installed in that runtime. Running calls retain admitted package settings
through updates, disabling or removal.

## Desktop runtime boundary

The panel uses the repository-pinned GPUI Kit script bindings, native controls
and semantic theme. Each mounted panel has a separate VM and disposable declared
resource cache. Project access goes through scoped Node commands, not cache paths.

Closing a panel, losing its Node or changing package revision releases its view
and subscriptions without cancelling admitted Node work. This is not an OS
sandbox: install trusted desktop UI code. Host and Mobile do not load the desktop
runtime; they use the Node's ordinary Skill/tool contracts.
