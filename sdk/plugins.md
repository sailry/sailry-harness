# Sailry plugin API v1

This is the v1 plugin contract for `github.com/sailry/sailry-harness`. Node
callbacks and Desktop entries run in the Go runtime; nothing described here
executes in the `--preview` desktop.

The current JavaScript contract includes `gpui-kit` module imports. The desktop
bridge maps those elements to MyGo and Kit; packages own their JavaScript business
logic and UI composition.

Desktop plugins import the host-owned `sailry/sdk` module. The host registers it for each mounted plugin and uses the existing Client → Node commands on local and remote connections. Do not bundle a private copy of the SDK, model response parser, or request recovery loop.

The desktop TypeScript contract is [api.d.ts](desktop/api.d.ts). The Go bridge checks those declarations against its exported methods. Headless callbacks have a separate [Node SDK subset](node/api.d.ts); desktop-only exports are not available there.
Their [model](node/models.d.ts) and
[installation](node/installations.d.ts) modules
share the scoped request contract with bounded snapshot observations.

| Capability | Public methods |
| --- | --- |
| Public configuration and scope | `readSettings()`, `context()` |
| Model catalog and selection | `listModels()`, `resolveModel(settings, catalog, field)` |
| Model request | `prepareRequest(command)`, `completeRequest(id)` |
| Durable requests | `prepareRequest(command)`, `completeRequest(id)`, `forgetRequest(id)` |
| Atomic Node mutations | `prepareTransaction(operations)`, `prepareNotification(content)` |
| Coalesced invalidation | `nextChange(cursor?, scope?)` |
| Model error handling | `errorCode(error.message)` |
| Plugin storage | `getValue(key)`, `listKeys(prefix?, after?, limit?)`, `setValue(key, value, revision)`, `deleteValue(key, revision)` |
| Sessions | `startSession(draft)`, `listTeams()`, `listAssociatedSessions(association, before?, limit?)`, `readSession(session?)`, `setSessionExecution(selection, revision, session?)` |
| Conversation | `readConversation(before?, limit?, session?)`, `readConversationRuns(before?, limit?, session?)`, `readConversationRun(turn, revision?, session?)`, `nextConversation(cursor?, projection?)`, `sendMessage(input, revision, session?)`, `stopTurn(turn)`, `setSessionConfig(config, revision, session?)` |
| Node HTTP | `requestHttp(request)` |
| Captured Git | `inspectGit()`, `prepareGitIndex(draft)`, `prepareGitCommit(draft)` |
| Declarative UI | `publishContributions(states)`, `nextContributionEvent()`, `closeContribution(id)` |
| Low-level context, resources and UI | Existing `sailry` exports: `context`, `theme`, `Header`, `Image`, `Conversation`, `header_action`, `next_change` |

Desktop and headless callbacks share the same session admission and execution commands.
`startSession({project, worktree, config, title, message, execution, team, models,
association})` prepares one durable request: complete it with `completeRequest(id)`
and retain that ID to recover an uncertain result. `sessions.start` permits creation;
`teams.read` permits `listTeams()`. Null `config` uses the execution Node's defaults.

`execution: "model"` runs the selected model without a delegation roster and rejects
team/member overrides. `execution: "team"` captures a team and its roles; omitted
execution retains the Default team behavior. A `team` selection is `{id, revision?}`.
An omitted or null revision resolves the latest team on fresh admission; a decimal
string pins an exact revision and rejects changes. Durable replay always returns
the originally captured turn. `models` optionally maps role IDs to
`{provider, model, effort}` overrides. `setSessionExecution(selection, revision,
session?)` changes future turns through the same session revision fence; sent turns
retain their immutable selection. Its wire command is
`set_session_team`, with `execution` and `models`; omitted fields use
Team execution and an empty override map. Team revisions in catalogs, sessions and admitted
turns cross JavaScript as decimal strings, including nested dispatch callbacks.

An optional immutable `association: {kind, id}` records a plugin's business identity,
for example `{kind: "schedule", id: scheduleId}`. The convenience methods stamp
`namespace` with the package name and reject a different namespace. Core commands
and dispatch callbacks require that exact namespace explicitly. Association metadata
does not grant project or file access. `listAssociatedSessions({kind, id}, before?,
limit?)` returns `{sessions, next_before}` with stable pagination and requires
`conversation.read`. An unscoped non-tool plugin may read or control its own associated
sessions with the corresponding `conversation.read`/`conversation.control` capabilities.
An existing worktree scope must still match; a captured session cannot be retargeted.
Pass an associated session ID as the final optional argument to read/send/configuration
methods, or bind the existing `Conversation.new` component to it. `nextConversation`
and `stopTurn` retain the captured view scope. Association lookup does not expose
another package's sessions or unrelated conversation history.

`nextConversation(cursor, 'runs')` observes connection, subscription generation and
run status/error changes from the shared Client snapshot. Content deltas leave
that call pending. The default projection preserves the full conversation view;
both projections retain the Node's access check before exposing session data.

`Image.new(id, {path, fit, width, height})` renders declared package images through the MyGo bridge.
`fit` defaults to `contain`; `cover` crops to the component bounds and `fill`
maps the image to those bounds. The bridge applies the selected object fit. Transparent
atlas composition can therefore share exact layer coordinates without changing
image alpha or implementing another renderer.
Optional positive pixel `width` and `height` set explicit image bounds and their
layout aspect ratio. Atlas layers can use these to avoid Kit inferring an
intrinsic source ratio from percentage heights during layout.

`sailry/forms.NumberField` reuses Kit's native number input over a `createText`
handle. `readText` retains the exact string; `nextTextEvent` reports `increment`
or `decrement` without converting the value to a floating-point number. Packages
own validation, exact arithmetic and saving. The bridge connects the retained
text handle and step events to the native control.

`sailry/forms.SliderField` binds a `createSlider(value, {min, max, step})`
handle to Kit's styled native Slider. `nextSliderEvent` reports local `change`
values and one `release` at the end of interaction; packages decide when to save.
`setSlider` updates presentation without publishing a user event. Release handles
with `releaseSlider`. The native slider owns its track, thumb and interaction state.

`sailry/ui.SettingsRow.new(id, {label})` reuses the application's settings row.
Pass an already localized label and place the trailing controls in its children;
Packages retain their drafts, validation and save behavior. The shared row
bridge supports standalone plugin settings pages without another settings sidebar
or a package-specific renderer.

`SettingsRow` also takes `description`, `icon` and `wide`. The description is a
muted second line under the label at the application's row metrics. `icon`
names a leading icon in a 24 px slot before the text: a bundled icon, or a
brand mark as `ailogo:<slug>` once the application carries that collection. A
name the application does not know draws no icon and is not an error. When one
row of a `SettingsGroup` has an icon, the group's other rows keep the empty
slot so their text stays aligned; a group without icons has no slot.
`wide: true` gives a select, input or slider a 256 px control that wraps under
the label on a narrow page, and takes no icon.

`SettingsGroup.new(id, {sortable: true, sort_label})` lets the user reorder the
group's rows. Each child gets a drag handle named by the localized `sort_label`
(required) for its tooltip and assistive technology. Dragging the handle moves
the row; with the handle focused, Space or Enter lifts and drops it, the arrow
keys move it and Escape puts it back. A drop that moved a row arrives through
`nextControlEvent()` as `{id: <group id>, value: {from, to, order}}`: the old and
new index, and the children's identifiers in their new order. The host keeps no
order: persist it and return the children in that order, as with any other
control. Children need stable identifiers. `sort_disabled: true` keeps the
handles in place, dimmed, while the order cannot change, such as during a save.

```js
import { SettingsGroup, SettingsRow, Menu } from "sailry/ui";
const group = SettingsGroup.new("services", {
  title: text.services, sortable: true, sort_label: text.reorder, sort_disabled: saving,
}).children(services.map(service =>
  SettingsRow.new(`service-${service.id}`, {
    label: service.name, description: service.detail, icon: `ailogo:${service.slug}`,
  }).child(Menu.new(`service-more-${service.id}`, {label: text.more, icon: "ellipsis", items}))));
// {id: "services", value: {from: 0, to: 2, order: ["service-b", "service-c", "service-a"]}}
```

A `Menu` with an `icon` and no `caret` is an icon button: its `label` is the
tooltip and accessible name and is not drawn. Pass `show_label: true` to draw
the label beside the icon.

Model prompts and response parsing belong to the package. `resolveModel` validates the selected model and reasoning effort against the Node catalog; credentials remain on that Node.

Use `sailry/models` for public `search`, `versions` and `resolve` with `models.read`.
`list({target:'node', kind?})` reads OCR, embedding and decision definitions with their
installation state from the captured Node. Node model identities are opaque strings.
`register({target, definition})` and `prepare(model)` require `models.install` and
return ordinary prepared request IDs. Complete registration first, then prepare
installation with `{target:'node', kind, id}` and its returned model identity.
Definitions retain pinned asset digests; OCR, embedding and decision target `node`, while
speech uses `{target:'controller', kind:'speech', id}` on its controller.
Headless callbacks cannot install controller-local speech models.

Node tools and callbacks import `decide` from `sailry/decisions` with the
`decisions.run` action. This read-only call accepts `{model?, state, questions}`;
`model` is an optional registered Node model ID. It uses the saved decision
selection when omitted and requires an explicitly installed model. Global decision
disablement also applies to explicit IDs. It does not generate conversation text
or download a model.

Questions use `type: "noul"`, `"choice"`, or `"score"`, with `instructions` and
optional binary criteria, named choices, or an ordered score legend. Binary answers
contain only their true probability as `noul`; choice and score answers include
normalized probabilities and their algorithm's confidence. Input is limited to
16 KiB of state, 16 questions, 16 choice options, 1 KiB of instructions and
256 bytes per option. Tokenized prompts must also fit the installed model context.

```javascript
import {decide} from 'sailry/decisions';
const result = await decide({state: pageText, questions: {
  save: {type: 'noul', instructions: 'Is Save available?'}
}});
```

`sailry/installations.read(ids)` observes only owned model installation requests
in their original captured scope and requires `models.install`. It retains the exact
prepared request IDs and exposes only their resource category, never another plugin's
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

`listModels()` includes an optional `api` field on each configured provider model
(`responses`, `anthropic`, `gemini`, or another model protocol) and its
`authentication` method. Packages can filter choices by protocol and login method
without receiving provider endpoints or credentials.

Pure host exports may be listed in `host.read_only`, a unique subset of
`host.handlers`. Invoke them with `prepareRequest({kind: "read_plugin_callback",
data: {handler: "read", input: {}}})`, then complete and forget the draft through
the same SDK. These reads have no durable receipt or retained request record;
Node denies mutations before dispatch. Omitted declarations retain ordinary
`call_plugin` admission. Writes and controls must keep that durable path and its
stable request ID. Reads retain their supplied package and scope: a concurrent
package change can reject a live read rather than switching it to newer code.

Authorized Code tools may use `prepareTransaction` with their own namespace's
`write` and `remove` operations. They retain the ordinary atomic batch budget,
revision checks and stable request ID. Plan and read-only callbacks cannot write;
a transaction grants no access to another package's data.
Flow callbacks return declared `call` transitions for mutations; do not issue
durable SDK writes from multiple evaluations of the same flow call. SDK write
ordinals belong to one evaluation. Keep a multi-batch capture and its cleanup in
one ordinary handler evaluation so every prepared write retains its own identity.

Flows and settings pages can use `isValueConflict(outcome)` to recognize an atomic private
key-CAS rejection with no committed effects. Only this Node-tagged failure can
continue with another declared `storage.transaction` step. Re-read current values
and recompute the package's deterministic delta, retaining business identifiers
in flow state. Each attempt has a separate durable request, within the existing
eight-operation limit. Configuration, authorization, permission, cancellation
and uncertain outcomes do not authorize continuation or automatic replay.
A `call` transition for a read-only operation may set `optional: true` for
supplementary data. Its failure, including a lost response, reaches the next
evaluation as `outcome` instead of ending the tool; stopping the tool still ends it.
After a key-CAS rejection, a final result may add `context` for the Agent to
reconsider changed data. The original fault and `isError` remain authoritative;
context must not claim an uncommitted write succeeded. Context uses the same
bounded JSON callback and protected service results as other package output.

`prepareRequest({kind: 'read_git_context', data: ...})` reads an `old` or `new`
source side using the same worktree, snapshot, path, metadata revision and strong
content revision returned by `read_git_change`. It requires `git.read` and the
captured repository scope. `offset` and `next` count source lines; concatenated
`text` pages preserve CR/LF and an unterminated final line without adding line
numbers. Binary, unsupported and oversized resources have an explicit
`skip_reason`. Working-tree reads still reject concurrent changes. A feature
that needs a retained snapshot must store its complete captured pages before
publishing them; these read commands do not create a persisted snapshot.

`nextChange(cursor, "package")` observes this package's inventory, private storage
and declared role, model or terminal catalog dependencies. Changes from other
controllers invalidate the same package without exposing storage keys or values.
The `"settled"` scope requires a captured worktree and observes completed tool
batches, completed runs and worktree changes. The `"repository"` scope adds
relevant filesystem and Git metadata changes in that worktree through Client's
scoped watcher. These scopes grant no resource-reading authority. Omit the scope
to retain the shared Node cursor.

Updates coalesce for 750 ms with a one-second maximum wait. Connection, visibility
and view-entry changes arrive immediately. The result includes `revision` for the selected event scope, `terminal_tools` for
terminal settings invalidation, and `references` for repository refs and identity
changes. These decimal string counters are invalidation tokens, not writable
resource revisions. Ordinary file edits preserve `references`; recovery advances it.
The result also includes `visible`; hidden
views keep their last invalidation and wait until shown, without refreshing their
data. Pass each returned opaque cursor to the next call. Use composer or
conversation observations for session execution and draft state.

Keep `id` while a request is unresolved. `completeRequest` observes the original durable receipt after a missing response; an unresolved receipt rejects with `unconfirmed`. A retry uses that same ID. A completed business fault is returned as `{Err: {code, message}}`; success is `{Ok: {kind, data}}`. Forget only confirmed requests, or drafts intentionally abandoned when a view or game closes. Closing a view does not cancel work already admitted by the Node.

Header actions accept an optional `disabled` boolean; the host applies it to the native Kit button.
An optional `menu` array uses the existing Menu item fields: `id`, `label`,
`icon`, `enabled`, `checked`, `separator`, `children` and `value`. The action
opens a Kit dropdown instead of emitting its trigger ID. Selecting an enabled
leaf emits that leaf's ID through `header_action()`; Header selections do not
emit Menu events or values. Without `menu`, flat actions remain unchanged.
Menus reuse the existing declaration validation and 256 top-level item limit.

## Runtime compatibility

Declare optional `min_sailry_version` inside `extensions.dev.sailry.platform`
as a complete SemVer value, including a prerelease when applicable. The execution
Node compares SemVer precedence against its own Sailry version. Omission imposes
no minimum. This field filters product versions; the extension's `api_version` selects its API contract.

When a package uses a new `sailry/*` module or export, extension field or permission,
set the minimum to the first Sailry version containing that capability. Publish the
dependent package version only after that Sailry version is released; earlier
applications do not enforce this field.

The Node rejects incompatible installation and enabling with structured
`PluginRequiresNewer {required}` fault details. An incompatible installed package
retains its files, configuration and saved enable choice, while its contributions
remain unavailable. Update inspection keeps compatible candidates separate from
newer releases requiring Sailry updates. Desktop's application update entry applies
to its local execution Node; a remote requirement identifies the affected Node.

## Icons

A `display.icon` (package, navigation, tool, connection, UI entry) is either
`{"kind": "Name", "data": "<name>"}` or
`{"kind": "Svg", "data": {"svg": "<svg ...>"}}` with the package's own markup.
Names come from three namespaces. Which one to use depends on where the icon
is drawn:

- The package's own `display.icon`, and a connector's, is drawn on a tile in
  the marketplace, plugin cards, plugin details, the updates dialog and
  connector tiles. Use `phosphor:` there, or `ailogo:` for a package or
  connector of one brand.
- Every other icon is drawn in the text color beside the application's outline
  icons: navigation and rail entries, pages, panels, settings sidebar entries,
  tools and tool rows, menus, commands, status badges and script `Icon`
  components. Use an outline icon there, `reicon:`. A `phosphor:` name in such
  a place still draws, but solid and heavier than its neighbors, and an
  `ailogo:` name draws its one-color variant.

The namespaces:

- `phosphor:<name>` is an icon of [Phosphor Icons](https://phosphoricons.com),
  for example `phosphor:database` or `phosphor:calendar-check`. Browse the set
  on that site and use the icon's name as shown there; the fill weight is
  implied and no weight suffix is written. Every icon of the set is available.
  It is a solid one-color glyph meant for the package's tile. The accepted
  names are listed in
  `internal/protocol/phosphor_icon_names.txt`. A package that names one sets
  `min_sailry_version` to `0.1.6`.
- `ailogo:<slug>` is a brand mark of the [AI Logo](https://ailogo.yldm.ai/)
  collection, for example `ailogo:github`. Use it for the `display.icon` of a
  package or connector that represents that one brand, and in view components.
  Navigation, panel and contribution icons do not accept it. Name
  only the brand the package is for; when the collection lacks it, use a
  Phosphor icon or the package's own SVG rather than another company's mark.
- `reicon:<group>/<name>` is a monochrome outline glyph from
  [Reicon](https://github.com/dqev/reicon), for example `reicon:time/calendar`.
  It follows the text color and matches the application's own icons, so it is
  the one for navigation, tools, menus and script `Icon` components. The
  accepted names are listed in `internal/protocol/icon_names.txt`. The
  application embeds the glyphs it and its bundled packages use; a listed name
  outside them is valid and shows a puzzle piece, so reuse a name an official
  package uses.

Marketplace cards, plugin cards, plugin details, the updates dialog and
connector tiles show the package icon on a rounded tile. A Phosphor or Reicon
glyph, or the package's own SVG, is drawn in white on a gradient of the package
`tint` (a tint chosen from the package name when none is declared). An
`ailogo:` brand mark is drawn on a neutral tile instead, in the brand's colors
where the collection has a color variant and in the foreground color
otherwise. In text-colored places (the rail, settings sidebar, tool rows and
menus) the icon declared for that place is drawn in the text color. A name
that nothing draws shows a puzzle piece. An
unknown `phosphor:` or `reicon:` name is not a valid navigation icon.

## Marketplace metadata

The v1 extension may declare `category` (`development`, `documents`, `data`,
`automation`, `entertainment`) and `tint` (`blue`, `violet`, `teal`, `amber`,
`gray`). These are package-owned presentation metadata, not native renderer or
permission selectors. Tool `display.locales` contains localized user-facing names;
tool descriptions remain model-facing text. Skill frontmatter accepts string
metadata keys `display-name` and `display-name.<locale>`. Readers fall back from
the requested locale to the source display name, then to the immutable skill ID.

`CHANGELOG.md` may contain `## <version>` sections; optional
`CHANGELOG.<locale>.md` files use the same headings. The Node and official catalog
generator select the section matching each inspected package version.
`Info.release_notes` and catalog entries expose `{text, locales}` as plain text,
without HTML execution or an arbitrary per-language display cutoff. The requested
locale falls back to English when its matching section is absent. Official catalog
entries include descending `versions` with each version's final package-content
`commit`, optional `min_sailry_version`, exact-version notes and Node-evaluated
compatibility. The Node selects the highest compatible candidate and acquires its
fixed commit. Details retain installed-version notes separately from catalog
history. Bundled, directory and other Git packages expose current-version notes;
only official catalog packages have catalog history. History is read-only and does
not add version selection or rollback. Existing package, response and resource
budgets still apply. `Mcp.host` contains only the endpoint hostname, never headers, credentials or the full URL. `Action.access()`
groups permissions for display; `read_only()` and execution checks are unchanged.

`SearchSkillCatalog {source, query, page, force}` is a read-only execution-Node
command returning `SkillCatalog {entries, page, next, total, query_scope, unavailable}`.
Sources are `official`, `skills_sh` and `skillhub`. Each entry carries actual display
metadata, its origin and an optional directory URL. A typed Git or registry source
is present only when that source can use the supported installation path. Web-hosted
entries remain visible without being misidentified as Git repositories. Listing
neither installs anything nor creates a durable request record. Installation still discovers and
validates the source on the selected Node: Git skills freeze a resolved commit;
registry skills retain namespace, version and signature verification.

Official plugin and skill listings share one cache. Online refresh resolves `main`
once and binds the index and online skill sources to that commit. An older catalog
without skills falls back to actual bundled skills, whose public source is resolved
by normal discovery. Plugin listings select the whole newer bundled entry when its
SemVer precedence exceeds the online version; they never attach newer metadata to
an older online source.

The external directories use their public browsing APIs, with Node-owned page
caching and short failure caching. Skills.sh browsing has real next-page information;
its search requires at least two characters and exposes no next page. A one-character
query filters only the requested leaderboard page, reports `query_scope: page` and
omits a global total. Skillhub uses its real page and keyword parameters. Directory
results describe available sources, not installed packages.

`ReadSkillDocument {source}` returns `{skill, content}` without installing or executing
instructions. Its `DocumentSource` is an already resolved Git commit and candidate
path, a verified registry coordinate, an actual bundled package and skill name,
or an installed package reference and declared skill name. Installed documents
use the exact retained package and configuration revision with the existing
immutable resource reader, including after update, disablement or removal;
they do not reread the editable source or grant execution authority.
Git directory links must first use discovery to select the real skill; display IDs
are not repository paths. Registry documents reuse namespace, signature and archive
verification. The existing single-document resource budget applies. This read does
not create a durable request record.

`ReadCatalogPluginInfo` returns `CatalogDetails`, with `plugin`, `skills` or
`unsupported` content. Portable manifests are validated from the repository's actual
bytes. Confirmed Codex and Claude Code packages expose their own metadata and any
supported standalone skills at the same immutable source; their hooks and other
unsupported integrations are never executed. Synthetic directory manifests cannot
replace repository files or bypass an invalid declared portable schema.

## Model channels

Declare channels in `extensions.dev.sailry.platform.model_providers`. Each preset
has an ID, display metadata and routes identified by their protocol, region and
product. IDs are scoped to the package. The common channel editor combines
builtins, public recommendations and enabled package declarations; packages do
not contribute a native settings renderer. A saved channel retains its stable
package name and preset ID. Each new turn resolves the enabled package and
captures its exact resources and request policies. Existing turns retain their
original capture.

Presets may share a source-scoped `brand` for one channel group. Optional
`brand_display` uses the existing navigation label, locales and icon contract;
explicit displays for the same brand must agree. Route names and saved preset
references stay independent of that group label.

Use `models.provide` for model adapters and `models.authenticate` for login
declarations. A private token-response transformation also requires
`credentials.inspect`. Installing and enabling a model package authorizes its
model-message and credential transformations. Operations still require the exact
provider/package binding and declared actions. Package updates preserve accounts
when their provider, package and authentication contract are unchanged; changing
an issuer, client, flow, injection or account scope requires sign-in again.
Historical captures and stored approval metadata remain intact.

Account token mappings may expose an optional `account_label` JSON pointer to a
public account email. Omit it when unavailable; names, token values, subjects
and account identifiers are never display labels. The shared credential
projection and channel list render this field for any authentication adapter.

`model_auth` owns declarative PKCE, device-code, token-exchange or start-and-poll
flows. A `script` flow composes the same services with a pure package step
function; it does not install an SDK, network client or another runtime in the
transformation sandbox. Declare its immutable `script`, exported `handler`,
fixed named `requests`, optional OIDC `issuer` and public `callback` metadata.
A callback declares its exact loopback `path`, optional pasted `redirect_uri`,
`manual` support and optional `allow_localhost`. An optional nonzero `port` fixes
the Desktop listener port; when combined with `allow_localhost`, its address uses
`localhost`. A fixed pasted address must match that port and path. If binding
fails and `manual` is enabled, Desktop continues with the declared pasted address.
The listener stays on Desktop for both local and remote Nodes; token exchange
stays on the selected Node. `refresh: true` explicitly enables
the refresh event. Named requests retain the existing endpoint templates and
private substitution rules.

The step receives `{context, now_ms, event, state}`. The Node supplies the
private prior credential, configured fields, callback address, registration and
PKCE proofs in `context`. Initial events are `start` or `refresh`; subsequent
events are `response` (HTTP status and document), `presented`, `callback`
(validated fields), `elapsed`, `identity` (verified OIDC claims) or `registered`.
A read-only `request` may set `optional: true`. Only declared GET endpoints accept
this flag; a transport or decoding failure returns a private `failure` event with
an error code so the package can retain already issued credentials. Cancellation
and the absolute login deadline still terminate the flow. HTTP errors remain
`response` events with their status.
A step returns one effect: `request` (declared name and private fields), `prompt`,
`callback`, `wait`, `verify`, `registered`, `finish` or `reject`. Effects carry
only their next private `state`; it never becomes a controller projection. HTTP
redirect refusal, response budgets, PKCE proof validation, OIDC signature,
issuer, audience, expiry and nonce checks remain in the shared Node owner.
The existing absolute login deadline and cancellation cover every step and wait.

The stable `chat_gpt`, `chat_gpt_oauth`, `claude_oauth` and `copilot`
authentication IDs can be declared through optional `authentication`. Their
existing private credential codecs stay unchanged. A `copilot` flow finishes
with `{kind, access, github, expires_at_ms, endpoint}`: the short-lived session
token, the GitHub token that renews it, and a service endpoint under
`githubcopilot.com`. Default inventory metadata binds existing accounts to
their authorization package without rewriting credentials or sent-turn captures.
Later admission checks the bound package, origin and authentication contract;
ambiguous declarations or changed bindings fail rather than silently changing
account ownership. Ordinary package code updates preserve accounts. Other model
authentication flows use the same interface with a `token` mapping and `inject`
declarations; native aliases finish with their retained credential representation.

The Node owns HTTP, callback state, deadlines, private persistence, refresh and
uncertain outcomes. The public login presentation selects the existing common
controller interaction. An optional `exchange` pure transformation may normalize
a private token response inside its dedicated authentication sandbox. Script
flows and credential transformations require `credentials.inspect` alongside
`models.authenticate`; they use ordinary package trust. Do not implement login
through an ordinary host callback, import another application's profile or retry
an uncertain token rotation. A possibly delivered rotation remains uncertain and
is never replayed automatically.

An authentication declaration may add named read-only `requests`, using the
existing endpoint format. `requestProviderHttp(provider, revision, name, input)`
resolves that name for the current channel and exact calling package. Plugin
accounts use only that installed package's own authentication declaration.

A package can declare native account reads separately with optional
`model_resources: [{authentication, requests}]`. Each authentication may appear
once; supported values are `chat_gpt`, `chat_gpt_oauth` and `claude_oauth`
(`copilot` has no account resources).
These declarations require `http.request`, expose no credentials, and cannot
declare mutations. The native authentication owner permits GPT resources only
at `https://chatgpt.com` and Claude resources only at
`https://api.anthropic.com`. It injects Bearer authentication and an actual
account identifier when available. Packages may declare fixed public headers
such as User-Agent or anthropic-beta, but cannot supply Authorization or
chatgpt-account-id. Resource reads use the existing credential renewal owner;
they do not change model authentication or require conversation access.
Endpoint URLs and headers are fixed declarations; public query and body values may reference
`{input.field}`. They cannot name grant fields or override credential headers.
The Node obtains and injects the grant through its authorization owner, applies
the existing HTTP time/size budgets, refuses redirects and rejects responses that
expose its protected credential values. This does not impose a public-IP-only
policy on explicitly configured self-hosted endpoints. These reads do not retain
durable request receipts and must not be used for state-changing operations.

A read-only resource endpoint may declare `cache_ttl_ms` from 1 to 900000.
The execution Node shares protected results and in-flight acquisition across
controllers and conversations, keyed by the effective credential revision,
calling package and resolved request. Authorization still runs on every read.
Only successful HTTP responses remain fresh for that interval; overlapping
failures are shared without replacing them with a prior success or delaying a
later recheck. Omit the field for live status checks. This policy never caches
mutations or general HTTP requests.

State-changing endpoints belong in the declaration’s optional `mutations` map.
`prepareProviderHttp(provider, revision, name, input)` prepares a stable request ID;
`completeRequest(id)` uses the ordinary durable admission and completion path.
The same ID returns its recorded outcome without repeating the HTTP operation.
Keep that ID when the outcome is uncertain; do not prepare a replacement request
automatically. Public provider references, operation names and nonsecret input
are retained with the request. Credential lookup and injection still happen only
at execution on the Node. Both provider HTTP methods are available in Desktop
and Node host callbacks and keep the same exact-package and revision checks.

A flow may declare an optional `warning`: `{text, locales}`, plain text of at
most 1024 bytes each without control characters and at most 32 locales. Desktop
shows it in a confirmation before every sign-in attempt of a channel using the
flow and sends nothing until it is confirmed; cancelling leaves the channel as
it was. It changes nothing on the Node and is not part of the account's
identity, so adding or rewording it keeps saved sign-ins. Name the target and
the consequence once. A mirrored controller shows channels without starting
sign-in, so it never shows the warning.

A channel without a sign-in has no attempt to warn before: a preset whose
`auth` is `none` or `api_key`, such as one served by a local program through
`model_adapters[].acp`. Declare the same `warning` on the preset in
`model_providers` instead, with the same limits:

```json
"model_providers": [
  {
    "id": "example",
    "name": "Example",
    "routes": [{ "protocol": "plugin", "endpoint": "https://example.com" }],
    "auth": { "kind": "none" },
    "adapter": "example",
    "models": { "kind": "discover" },
    "warning": {
      "text": "This uses your own Example account; heavy automated use may count against its limits or violate its terms",
      "locales": { "zh-CN": "..." }
    }
  }
]
```

Desktop asks it once when the user picks the preset to add a channel, before the
channel form: Continue opens the form, Cancel adds nothing. Editing a saved
channel does not ask again. On a preset that has a sign-in, the flow's `warning`
is the one shown before each attempt; the preset's is used only when the flow
declares none. The warning needs no action and changes nothing on the Node.
`model_adapters` declares a script, allowed HTTPS origins, response framing
(`sse`, `ndjson` or `json`), schema dialect and explicit capabilities. A preset
using one must select the `plugin` protocol. Its pure exports translate neutral
model data: `prepare` produces an HTTP request description, `decode` produces
validated deltas, and optional `classify` maps an upstream failure. Without it,
Node applies the native HTTP classification for authentication, permission,
rate limits and server failures. Optional discovery
uses `prepare_discovery` and `decode_discovery`. See the authoritative
v1 wire types and
authentication declarations.
Do not infer model capabilities from IDs or synthesize fallback context limits.
Discovery can advertise `capabilities.features` as a list of `{id, name?, icon?,
options: [{id, name?}]}` descriptors. IDs are stable parameter values; display
names do not affect requests. Selected choices arrive in `prepare` as
`features: Record<string, string>`, independently of reasoning. An empty map
means no feature is selected. A translation maps only its own advertised choices
to the upstream protocol. Native Responses currently maps `speed: "fast"` to
`service_tier: "priority"` when that choice is advertised by the model catalog.

`prepare`, `prepare_discovery` and `decode_discovery` receive `settings`: the
installed package's resolved non-secret settings, read when the request is
captured, or an empty object. `decode_discovery` may return
`capabilities.options` for a model: at most 16 named JSON values of at most
4096 bytes in total. Sailry saves them with the model, replaces them at the next
discovery and passes them back in `prepare` as `options`; a provider policy of
the same name wins. They carry facts a translation needs later, such as a price
read from the catalog. Discovery runs only when the user updates a channel's
models or the channel is validated, never on a timer, so a translation that
depends on such a fact records when it was read and treats an old one as
unknown. `prepare` may return `{refusal: {class, message}}` instead of a
request. Nothing is sent and the turn fails with that class and message.
Pure modules declare their entry and resources under `dev.sailry.platform/`.
The inert `sailry/adapter` module exposes `version` and `parseJson`; its
[type declarations](node/adapter.d.ts) describe
the neutral input and deltas. These modules have no `sailry/sdk`, filesystem, network or tool dispatch. The Node performs
transport and credential injection; model translation never receives tokens.
Model callbacks use the shared 60-second script execution limit, including JSON
conversion, with cancellation and a 64 MiB callback allocation budget. moejs
enforces its recursion bound and reports excessive recursion as a script fault;
there is no configured physical stack-byte limit. Model requests remain bounded
to 32 MiB. The script deadline does not bound HTTP/SSE waiting or authorization I/O.
The Sailry Node remains the sole model/tool execution and history owner. Package-owned tests
can use `translationInput` and `translate` from `scripts/test-host.mjs` with an
explicit current `SAILRY_PLUGIN_TEST_HOST`; that bridge runs the production pure
VM without installing an SDK.

### Agent programs

A package may instead provide models through an installed command-line agent
that speaks the Agent Client Protocol. The Node owns the process and the
protocol; the package only declares. Packages get no process or standard-stream
API. The agent is used as a plain model: its own tools are refused and Sailry
keeps the loop, tools and history.

`programs` (at most four) requires the `process.run` action:

| Member | Meaning |
| --- | --- |
| `id` | Name within the package |
| `command` | Executable name, resolved on the Node's search path. No path, no shell |
| `args` | Up to 32 fixed arguments. `{dir}` is the program directory, `{model}` the selected model |
| `env` | Up to 8 variables the Node does not own. `{dir}`, and `{key}` for the channel's API key |
| `files` | Up to 8 files of at most 64 KiB, written into the program directory before each start |

The program directory is the package's own
`programs/<id>` data directory and the working directory. The program inherits
only the search path, account directories, locale, temporary directory and proxy
variables. The Node starts it on demand, stops it after ten idle minutes and
kills it with its children when the package changes, is disabled or removed.

A model adapter names a program with `acp` instead of `script`. It declares
`framing: "ndjson"`, `origins: []` and no `files` capability:

| `acp` member | Meaning |
| --- | --- |
| `program` | The `programs` entry to run |
| `replay_bytes` | Largest prompt in bytes, 4096 to 8 MiB. A larger history is a context overflow |
| `sign_in` | The instruction shown when the agent reports that it is signed out |
| `system_meta` | Dotted `_meta` path of `session/new` that takes the system text. Without it, or when the agent does not advertise it, the system text opens the first prompt |
| `efforts` | Effort level (`minimal` to `max`) to model identifier suffix. Variants fold into one model |
| `model_argument` | The model is the program's `{model}` argument, one process per model. Required exactly when an argument has `{model}` |
| `auth_method` | The `authenticate` method the agent is asked to use once when it requires authentication |
| `failure_markers` | Up to four texts that mark a reply as a failure |

The preset uses a `plugin` route, `models: {"kind": "discover"}` and `auth`
`none` or `api_key`. Its `endpoint` is required and is not contacted. Models are
the ones the agent lists; none is invented.

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

Use the shared script controls and their `transition` API for position, size and opacity; the bridge renders them with MyGo. Plugin scripts sequence game events with `cx.sleep()` and `cx.notify()`; they do not need a per-frame rendering loop. Keep game rules and animation sequencing in the plugin.

`SegmentedTabs.new(id, {items, selected, disabled, fill})` uses native Kit tabs.
Set `fill: true` for equal-width choices across a modal form; it defaults to
false so page and toolbar tabs retain their content widths. Selection events
retain the same `{id, value}` payload.

Icon names in view components and menu items also accept `ailogo:<slug>`, a
brand mark of the [AI Logo](https://ailogo.yldm.ai/) collection the Desktop
links: `ailogo:openai`, `ailogo:claude`. The slug is the collection's own, in
lower case. A suffix chooses a variant where the brand has it: `-color` (the
mark in its own colors), `-text` (the wordmark), `-text-color`, `-text-cn`,
`-brand` (mark and wordmark) and `-brand-color`; without a suffix the mark is
one color. The `Icon` component draws a `-color` variant in the brand's colors
and every other variant in the text color. Settings rows also preserve explicit
color variants; menu items, table lines and other tinted places draw a one-color shape. A name the collection
lacks behaves as any unknown icon name does. Existing names are unchanged, and
a manifest's navigation icon still takes a `phosphor:` or `reicon:` name or its
own SVG. Brand
names and logos are trademarks of their owners: use a mark only to identify
that owner's product or service, unmodified, and not to suggest endorsement.

`confirmAction({title, detail, action})` from `sailry/ui` shows the application's OS-native warning dialog with Cancel and the given action label. It resolves `true` only when the action is chosen; a closed view, missing window or another open prompt resolves `false`. Use it for actions that affect state outside the package, such as switching a shared checkout.

`Loading.new(id)` from `sailry/ui` is a nonvisual pending marker. Reads load
silently without a spinner or input-blocking overlay. Keep the page heading,
toolbar and scroll container mounted, and retain existing content during refresh.
Disable actions that require unavailable data; show an empty state only after a
successful read.

`CardList` handles these states automatically. Its children are the row-content
slot: existing children always remain visible; without children, `loaded: false`
stays empty and a successful read shows `empty_label`. Settings lists may opt into
`loading: true` to show a first-read skeleton card; this never covers existing
rows or adds a floating overlay. Omit the state props to use the original
layout-only composition. The plugin still
owns requests, successful-read state, drafts and error reporting.

```js
import { CardList } from "sailry/ui";
const content = CardList.new("records", {
  loaded,
  empty_icon: "inbox",
  empty_label: text.empty,
}).children(records.map(renderRecord));
```

`ComposedButton.new(id, { label, tooltip, size })` from `sailry/ui` reuses Kit's native ghost button for image targets. Optional `size` sets a square in pixels; activation arrives through `nextCardEvent()`.

Wrap a native focusable leaf with `FocusEntry.new(id)` from `sailry/ui`, then call `focusEntry(id)` and `cx.notify()` from a selection event. On rendering, the bridge focuses the first eligible native descendant; if none accepts focus, it focuses the wrapper. A newer request or `focusEntry(null)` replaces the pending request. Packages select the visible leaf and retain their own binary or collapsed-content fallback.

Navigation and resource entries use `desktop.navigation` and `desktop.panel`. Scripted surfaces declare their JavaScript entry and resources. The host resolves entries from the captured Node inventory and immutable package; packages do not gain native privileges by choosing a familiar name. Standard conversation controls remain host-owned.

Workspace and scripted settings pages retain a mounted JavaScript runtime and native state for their captured Node and resource scope. Ordinary navigation reuses that mount and its drafts; returning signals entry refresh. Hidden pages pause visibility-bound work. Package replacement, removal, invalid scope and Node changes invalidate the mount; supported settings refreshes can retain it under the same package digest. Releasing a view does not cancel already admitted Node tasks.

Use `Workspace.new` from `sailry/ui` with children in this order: main content,
optional details, optional resource navigation. `has_details` controls the right
column's availability; `navigation: "resource"` places the third child in the
Shell's left sidebar. The sidebar shares the application width, resize handle
and navigation toggle. `min_navigation_width` is bounded to 0–320px, within the
application sidebar's 200–320px range. Details keep their width and open choice
per Workspace, separately from conversation panels. `min_details_width` is
bounded to 0–480px (default 260); `default_details_width` applies on first
projection, bounded to 260–480px (default 400). `details_label` names the right
column's toggle. The Shell owns header alignment, dividers and visibility. Call
`showWorkspaceDetails()` after an explicit selection to reveal a previously
hidden sidebar without changing its resized width. Do not call it on every render.

`PanelHeader` has a fixed 48px height, or 32px for `size: "row"`. `bordered`
draws its bottom edge; `surface: "content"` fills the content background.

A scripted settings page reads its immutable bundle and mounts on first use. Section switches reuse the mount while its captured scope remains valid. A settings page without a script uses the shared schema form.

`permissions` contributes standard OS permissions to **Settings → Permissions** without a
settings page. Each entry declares `kind` (`full_disk`, `accessibility`,
`screen_recording` or `microphone`) and a stable authorization `subject`, such as
an application bundle identifier. Entries for the same execution device, subject
and kind are deduplicated; rows aggregate all subjects of one kind. Disabling or
removing a package removes its contributions without revoking OS grants.

A declaration with subject `sailry` and no callbacks uses the controller's native
handler. A different subject declares `check` (a `host.read_only` export), optional
`request` (a host export), and the SDK `actions` needed by those handlers. The same
actions must appear in the extension's capability list. `request_group` identifies
one onboarding flow covering multiple permissions for that subject. Each explicit
request handles only missing grants and deduplicates that group for the current
operation; failures and cancellation never imply authorization.

Callbacks receive `{kind, subject}` and return
`{grants: [{kind, authorized: true | false | null}], can_request: boolean}`.
Return the current grants, including the requested kind; `null` means detection is
unavailable. The host binds the result to the captured package, device and subject.
Permission callbacks use the dedicated `permissions` surface: only declared
check/request handlers and their declared SDK capabilities are admitted. This
does not grant a settings page, unrelated callbacks or disabled-package access.
Read-only checks cannot request OS permissions. The Node SDK exposes
`readComputerPermissions()` and `requestComputerPermission("accessibility" |
"screen_capture")` for the captured computer helper; plugins map those service
results to standard permission kinds and own any onboarding policy.

`settings_page` contributes settings navigation, placement and an optional shared enable control. A script page supplies `entry`; a schema-generated page omits it. Feature pages and workflows belong in the package, including bundled code-index and search settings. The host provides Kit composition and scoped service APIs, never package-specific native page selectors.

String settings may declare a JSON Schema `pattern` alongside length limits. Invalid
patterns reject the schema; values that do not match are rejected before saving.

A settings schema may carry `x-sailry-locales`: per locale, a map from a field's
name to its translated title. A map keyed by the English title is still read.
A public string setting may declare `x-sailry-time: true` to use Kit's minute-precision
time control in the common configuration form. Defaults and saved values use strict
24-hour `HH:mm` from `00:00` through `23:59`, without an implicit time zone conversion.
Boolean settings use the same form's switch and ordinary settings save path.

A successful `SavePluginSettings` publishes `settings.changed` in that package's
dispatch namespace within the same transaction. Its payload contains only the
public `package` reference and decimal-string `settings_revision`; the reference
uses the same string revision. Subscribe through an ordinary dispatch handler and
read current settings in its callback. Invalid saves publish nothing, and replaying
the same request does not repeat the event. Callback failures have their own job
outcomes and do not replace the saved settings receipt.

The execution Node owns child concurrency. A retained `x-sailry-child-concurrency` marker is accepted for compatibility but binds no policy; the Node reads its own setting when claiming children, with separate running budgets and FIFO order for each main session.

`searchCode(query)` and `outlineCode(path)` require `code.index.read` and use the
current worktree through the shared Protocol and Client boundary. Search accepts
literal terms plus optional name substring, path prefix and language, returning
20 symbols by default and at most 100. Outline parses only its requested saved
file when needed and returns at most 256 declarations. Both report the index
revision and `partial` / `truncated`; symbols carry full-content `file_revision`
and zero-based ranges with UTF-8 byte columns and exclusive byte ends. Actual
per-file capabilities and unsupported states must remain visible. Grammar support
is source structure, not type analysis or compiler diagnostics.

`readFileLines(path, offset, limit, expectedRevision?)` requires `files.read` and
reads the current worktree through the same confined file service. Offsets are
zero-based and returned line numbers are one-based. The Node compares the complete
file's BLAKE3 revision before slicing when `expectedRevision` is supplied. A
revision conflict returns no source from the changed file. The existing 128 KiB
`readFile` preview is not complete index input. Indexing reads a complete bounded
file separately, with a 1 MiB default limit.

The `code.graph` flow operation also requires `code.index.read`. It reads the
`callers`, `callees`, `context`, `impact` or `map` view of name-resolved references
for an exact name, a single path and zero-based line, changed paths or map focus
paths. Pages are bounded to 200 items with offsets; impact follows at most three
reverse hops. Each reference reports a `local`, `imported`, `directory` or
`workspace` confidence and whether several definitions share that tier.

The `code` package composes `code_context` with ordinary `code.search`,
`code.graph` and `files.read_lines` flow operations. It selects at most 20 symbols,
the first symbol's callers, callees and impacted files, and five merged
source snippets within 16 KiB of UTF-8 JSON, preserving file/index revisions and
incomplete-result flags. Its graph step is optional: a failed graph omits the
graph fields and sets `graph_unavailable` to a short reason. File reads check
each selected revision; stale ranges never expand against new source. Abandoning a Client search or outline sends its
scoped `cancel_code_index_query` while the transport runtime remains available.

`media.settings.read` and `media.settings.write` authorize
`readMediaSettings`, `listMediaModels` and `prepareMediaSettings` on the
captured settings surface. Image inspection and image and video generation are
built-in tools (`inspect_image`, `generate_image`, `generate_video`): the
actions and handler operations `media.inspect`, `media.image` and `media.video`
still validate, but a declaration that names one binds no tool.

`code.index.settings.read` and `code.index.settings.write` expose the Node index
enable setting on the captured settings surface, without a worktree, session,
turn or callback invocation. `web.settings.read` and `web.settings.write` retain
search settings and driver checks. Disabled packages can configure their own
settings surface. Use `sailry/search.prepareSettings` with opaque
`sailry/credentials` field handles for search credential updates; plaintext
credentials never enter JavaScript.

A Host package can declare `enabled_service: "code_index"` with `code.index.read`
to bind its switch to the Node code index. Only one installed package may own a
service binding. The inventory owner applies the enable choice atomically and
preserves it through package updates; the field grants no actions. Retired
`language` bindings and `code.*` language-server capabilities remain readable only
inside historical captures and cannot be installed or executed.

Game prompts, legal-move selection and response validation belong in each game package. Use `prepareRequest` with `generate_plugin_text`, then `completeRequest` and `forgetRequest`; the core model API does not construct game prompts or interpret moves.

The browser package owns annotation review and note drafts. `browserAction({kind: "annotate"})` toggles the shared element picker, which stays active until Escape; `secondary-shift-c` in the browser toggles it too. A plain click or Enter inserts the element into the captured composer as a chip, Shift selects several elements for one commit, and the arrow keys move to the parent or child. The shared service validates each capture, crops its screenshot and stages the capture with a compact JSON page context marked as untrusted; capture failures surface as `annotation_error`. An Option or Alt pick produces a review capture instead: `readBrowser()` and `nextBrowserChange()` expose its `{token, element}` and capture error; `BrowserAnnotationImage` displays its protected crop without exposing image bytes to JavaScript. Elements carry the page title, selector, ancestor path, tag, identifier, classes, allowed attributes, accessible name, bounded text and markup, selected computed styles and optional development-build component metadata; they never carry input values. Compose the review with Kit controls, then call `submitBrowserAnnotation(token, reference)` with the JSON reference through `sailry/ui`, or cancel with `browserAction({kind: "cancel_annotation", token})`. The shared service verifies the captured element, token and note bounds before inserting the chip and note into the original composer's draft; navigation or view changes invalidate that capture. `BrowserSurface.new(id, {covered: true})` hides the OS child view behind package overlays without unmounting or cancelling capture; coverage defaults to false. Surface and preview components bind the mounted view lease, so older panels cannot change coverage after remounting.

Knowledge retrieval is a shared Node service rather than a bundled feature plugin. Notes, reminders and other feature packages organize their own collections and expose their own settings and conversation tools using the storage and knowledge operations in the v1 contract. The built-in knowledge settings configure vector models only.

`ResourceTree.new(id, { items: JSON.stringify(items), selected, current })` takes a serialized `ResourceTreeItem[]` as its `items` field. The native bridge decodes and validates this recursive model and retains its tree state, events and native menus.

Optional `guides: true` adds dashed hierarchy guides to resource rows, following `indent` and `skip_depth`; it defaults to false and leaves tree interactions and the branch variant unchanged.

`PathBar.new(id, { segments, disabled })` from `sailry/ui` displays a path of
string segments. Every segment, including the current one, is a native Kit
button; long labels truncate within the available width. A click emits
`{id, value: index}` through `nextControlEvent()`, where `index` is zero-based.
`disabled: true` keeps the path visible and prevents activation. The package
owns the mapping from segment indices to paths and any optional path editor.

`Terminal.new(id, {terminal})` mounts a terminal from the surface's captured Node
and resource scope. `releaseTerminal(terminal)` from `sailry` synchronously stops
that surface's mounted input and screen observation without closing the Node
terminal or replaying input. Release the binding before dispatching an explicit
close, and keep the component unmounted while the close is pending or uncertain.
A confirmed failure can mount it again under the same component ID to recover
the retained Node screen. `focusTerminal(terminal)` focuses a mounted terminal.

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

Declare `ui` entries in `extensions.dev.sailry.platform`: a slot (`composer`, `model`, `context`, `status`, `statistics`, `footer`, `provider`), kind, localized label, icon, order and named handler. An optional `desktop.ui_entry` runs the state/event module. Except for Node-owned Status state, publish state through `publishContributions` and consume actions through `nextContributionEvent`; responses include the event sequence in `reply_to`. Actions stay disabled until their matching reply, while searches can supersede earlier searches. The host renders normal and compact controls from the same declarations. Controls with `overflow: "menu"` appear in composer settings, including context controls; embedded conversations also place their context controls there. Metric details join the shared statistics hover surface; the compact statistics menu remains accessible by keyboard. A `statistics` metric may declare a handler to render its package-owned details in that surface; passive metrics keep their flat declared details. A package declares at most one custom detail surface.
Composer placement uses the same declarations for the toolbar and settings:
below 680 logical pixels secondary composer controls move into settings; below
440 context controls move there as well. Indicators remain beside the explicit
model selector and send control. Placement does not change contribution values.

A `menu` contribution renders its `choices` as a Kit popup menu. The string
`value` names the checked choice. Consecutive choices with the same `group` share
a section and groups are separated. A choice `description` appears as muted text
on the right of its row. A choice with `children` opens one submenu level; the
parent row is not selectable and children cannot nest further. Children count
toward the 256-choice limit and every id is unique across the whole menu. Only
`menu` contributions accept `children`; Select and Picker choices stay flat.

The `status` slot supports Buttons and Popovers computed by the execution Node,
including with no Desktop connected. A Host package declaring Status entries must
declare `host.status` and require Sailry `0.1.0-alpha.9` or later:

```json
{
  "handlers": ["badge", "act"],
  "read_only": ["badge"],
  "status": {
    "read": "badge",
    "invoke": "act",
    "changes": ["package", "conversation", "repository"]
  }
}
```

`read` names a declared read-only export. It receives `{}` with the ordinary
captured Composer context (`package`, `worktree`, `session`, `turn: null`,
`invocation: null`) and returns an array of
[`StatusState`](node/api.d.ts) objects.
Omitted Status declarations become hidden. The Node validates declared ids,
uniqueness, the 32-entry limit and a 64 KiB serialized state limit. Status state
uses localized `label` and `segments`; `details: [{label, value, progress?}]`
supplies Desktop tooltips and Mobile sheets. Detail values are at most 4096 UTF-8
bytes and optional `progress` is a finite percentage from 0 through 100. Keep
`value` null and omit `scope`, `choices` and `reply_to`; Popover open state is
controller presentation, not package data. `publishContributions` rejects Status
ids because the Node owns those states.

`changes` selects the package's private-value/settings changes, the captured
session's run/tool changes, or the captured worktree's filesystem/Git changes.
Conversation and repository signals are ignored without their corresponding
scope. Inventory changes always refresh the package set. The Node shares and
coalesces reads across controllers, retains the last valid state after a read
failure, and recomputes on resubscription after restart.

Optional `invoke` names a declared export outside `read_only`. A Status Button
uses it through durable admission with `{id: "<contribution-id>"}` in the same
captured scope; its return value may be any JSON. Completion refreshes the
package's Status state. Controllers retain the request identifier for an explicit
retry after an uncertain outcome and never replay the action automatically.
The contribution's `handler` remains its Desktop event name. Without `invoke`,
Desktop forwards its ordinary click/open events to `desktop.ui_entry`; a Status
Popover still requires that entry for its package-owned panel. A Button without
an invoke export or Desktop entry is display-only. Mobile presents the localized
details, with an action only for Buttons declaring `invoke`.

Desktop keeps Popover open state local. Status events need no state publication
or `reply_to`. Use `closeContribution(id)` after a confirmed package action when
the panel should close; it accepts only this mounted package's Status Popover id,
closes that controller's panel, and neither publishes Status state nor invokes an
action. Unsubmitted Composer drafts do not change Node-owned Status state.

A composer `indicator` publishes `{percent, loading, tone, hint, open}`. `percent`
may be null for an unknown measurement; `open` defaults to false. With an optional
named `handler`, it uses a Kit button and controlled popover containing the
package's `desktop.ui_entry` view. Consume boolean `change` events and acknowledge
their sequence with `reply_to` while publishing `open`. A package may have only
one panel-bearing contribution: a popover, an interactive indicator or a metric with a handler.
Passive indicators cannot publish `open: true`.

With `conversation.read`, `readComposer().statistics` exposes Node-owned context
observations and their `context_limit`, `context_model` and `context_timestamp_ms`,
plus an optional post-compaction `context_estimate`. The separate `budget` is the
current effective policy, including its model, capacity, compaction trigger and
output reserve; it must not replace an older observation's capacity. Exact
integers cross this script boundary as decimal strings. Recent model
observations retain their request and turn identities; the optional `turn_index`
is the one-based task round in this conversation, not a model-call count. It is
derived from canonical visible history and stays absent for non-task requests.
Multiple requests or recovery attempts in a turn share that round. `last_compaction` is a
canonical reference independent of the loaded history page. Its `id`, `turn` and
`turn_sequence` can be passed as
`revealComposerEntry({entry: id, turn, turn_sequence})` to locate it in the captured
composer. The host validates the current Node/session/package scope and reuses
Client's history loading. This read-only navigation never changes session
configuration or starts compression.

The `footer` slot accepts passive `metric` contributions for the current model
account, independently of Sailry session statistics. Desktop displays them in a
separate sidebar row above host performance while that conversation is selected.
Only contributions matching its current provider are shown. Supply
`value` and localized `details: [{label, value, progress?}]`; optional `progress`
is a finite percentage from 0 through 100, including decimals. The shared metric
popover lists rows without `progress` first, then rows with `progress` as labeled
Kit progress bars, each group in declared order. Long labels truncate so values
stay whole. A `metric` declaration may set `width` from 240 through 480 logical
pixels; the host default is 300. Packages do not supply popup views, positioning
or styles.

A package declaring `models.provide` or a matching native `model_resources` entry
can use `readModelContext()` and
`nextModelContext(cursor)` without `conversation.read`. This projection contains
only `cursor`, `connected` and the selected current channel's
`provider: {id, revision, model, authentication, preset}`, or `null` when the
selection is unavailable or does not match the package declaration. Native
providers have a nullable `preset`; plugin providers retain their exact package
binding. Revisions and cursors are decimal strings.
It exposes neither credentials nor conversation content. Clear displayed data
when the context changes and discard HTTP replies for an older selection.
`requestProviderHttp(id, revision, request, input)` invokes a named authentication
request through the execution Node; the package receives the public HTTP response
without receiving its private authorization headers. It does not admit a durable
command or create a session usage record.

The `provider` slot accepts only passive `metric` contributions in channel rows.
The host mounts one contribution controller per enabled package for the visible
channel page using the existing `desktop.ui_entry` (`context().surface` is
`provider`). `readProviderContexts()` and `nextProviderContexts(cursor)` expose
`{cursor, connected, providers}` for all current channels matching that package's
preset or native resource declaration. Each provider has the same public fields
as `ModelContext.provider` plus `scope: {provider, revision, credential: {node, id}}`.
The scope revision is the `providerRevision:credentialRevision` token; the provider
revision used for HTTP remains unchanged. These are public references, not authorization
values. Publish that exact `scope` with
its metric state; the host rejects stale scopes and keeps identical contribution
IDs for different channels separate. Compact row details reuse localized labels,
values and optional progress. A metric without details renders its icon and value.
Missing measurements should be hidden. Keep one package refresh loop, discard
replies for replaced identities, and use `requestProviderHttp` for all reads;
the execution Node shares matching authorized requests with conversation footers.
Leaving the page releases its host, subscriptions and outstanding read observers.

The `model` slot places ordinary contribution controls in the model popover.
Its state segments also appear beside the model label; `accent` uses the semantic
primary color. Labels, selection and visibility remain package-owned.

Declare public `conversation_options` with the existing settings schema subset.
Values live under `SessionConfig.options[package]`, share the conversation's
revision and freeze at turn admission. `readComposer()` exposes only the calling
package's options with its declared defaults. Use
`setComposerOption(cursor, 'plugin', {key, value})` for the same draft/configuration
path, including before a session exists. The Node validates changed values and
rejects cross-package writes. The `turn.initialize` input's `options` field
contains only that package's captured namespace. An explicit `conversation.read`
declaration retains access to the existing complete public configuration;
conversation options are not secrets. Removed option keys remain stored without
becoming executable.
A field may declare `x-sailry-import` naming a retired public configuration field.
Installation imports explicit current values once, without overwriting an existing
namespaced value or rewriting historical revisions. Imports grant no authority.

Host packages can contribute `conversation_modes` to the common mode selector:

```json
{
  "id": "review",
  "option": "mode",
  "display": {"label": "Review", "locales": {"en": "Review"}},
  "permissions": {"write": false},
  "instructions": "Inspect the requested changes and report findings"
}
```

The named `conversation_options` field must be a string with an empty default and
an enum containing `""` and the IDs of modes using that field. Each package can
declare up to 16 distinct modes. `permissions.write: false` requires the effective
Plan mode; `true` uses Standard and still needs the session's ordinary tool permission.
Stricter inherited Plan restrictions remain valid. Instructions configure the main
coordinator; delegated children keep their task, role and captured restrictions.
Node validates the selected declaration before configuration or turn admission.
Exactly one package mode may be selected. Changing modes uses the existing session
revision; returning to Standard or Plan clears active mode options while preserving
other choices. Disabled modes reject new admission; removed declarations become
inactive without deleting stored options or rewriting captures. Turn instructions
come from the captured package, including packages that provide only modes.

`readComposer()` includes `modes`, the current `mode_selection: {package, id} | null`
and the effective `mode: 'code' | 'plan' | 'flagship'`. A package with `conversation.control` can
select its declared mode with `setComposerOption(cursor, 'mode', {package, id})`,
or select the builtins with `'code'`, `'plan'` or `'flagship'`. The host atomically clears other
active mode choices. A mode cannot replace a captured assistant with another
revision of the same package. Standard handles tasks directly and permits delegation
when the user explicitly requests it; the core Flagship mode owns the optional
autonomous coordination policy. Retained `proactive` values do not select that mode.

Tools can bind public operations (`progress.update`, `storage.get/list/set/delete`, `settings.read`, `http.request`, `artifacts.write/read/list`) without an MCP server. Declare each operation's required action. Node owns approval, planning restrictions, cancellation and durable outcomes; a desktop JavaScript callback is not an Agent tool implementation.

`files.apply` binds only as a tool handler's single `operation`, requires the
`files.write` action and Sailry 0.1.5 or later (`min_sailry_version: "0.1.5"`).
The handler returns `{input}`, where `input` is the whole Codex-format patch from
`*** Begin Patch` to `*** End Patch`. The Node parses it, requires a current read
of each updated, moved or deleted file, validates up to 16 entries and requests
one approval. Flows and `prepareRequest` cannot use it.

The main agent starts with everyday workspace tools; other package tools stay
discoverable through `tool_search`. A tool can declare `"initial": true` to start
visible as well. It is accepted only for tools whose effects stay inside the
package: a handler declaring `read_only`, or tools whose declared operations are
all `storage.*` operations. MCP tools and tools with file, process, network or
other external operations reject it. Delegated agents and role tool selections
keep their own rules.

An `mcp.json` server can declare `tools`, a list of up to 64 distinct server tool
names, to expose only that subset (Sailry 0.1.5 or later). Unlisted tools are not
registered, counted or callable; listed names the server does not offer are ignored.

A `host.turn.initialize` hook receives the turn's user text as `message` when the
package declares `conversation.message.read`. It contains only the text, without
attachments or references, capped at 4 KiB of UTF-8 on a character boundary. It is
`null` for automatic, notice and delegated turns and for empty text. Turn hooks
can write the package's private storage with an identity derived from the turn
(`initialize`) or the newest observed tool result (`before_model`), so a replayed
evaluation reuses its request. Planning turns still reject these writes; treat
any write failure as optional and continue.

`initialize` returns `{tools, instruction, parts?, parameters?}`. Both `instruction`
and `parts` become agent-only turn context, never part of the system prompt; put fixed
guidance in the package's `instructions` so the system prompt stays byte-stable.
`parts` lists up to 8 nonempty strings, 32 KiB of UTF-8 in total, and repeats are
removed. The Node persists a part only when no agent-visible turn context of the
conversation already holds that exact text, so an unchanged part is stated once and
appears again only after compaction hides its earlier copy. A turn that needs no
context returns an empty `instruction` and no parts.

Packages that submit automatic input with `ContinueTurn` can declare
`continuation_marker` in `extensions.dev.sailry.platform`, using a localized
`label` and optional `locales` map. Node captures the package reference and marker
in `conversation::Run.continuation`; controllers display this as a system marker
instead of a user message. The model still receives the submitted input. Without
a declaration, controllers use their generic continuation label. `Run.origin`
continues to identify inherited history independently of automatic input.

A package may declare up to eight further markers in `continuation_markers`, a
map from a marker id to the same localized label. A `continue` operation selects
one with `marker: "<id>"`; an undeclared id is rejected. Without `marker` the
continuation carries `continuation_marker`.

Automatic turns are bounded. A session starts at most eight notice turns and
continuations in a row without user input; any user message, guidance included,
starts a new count. A package that owns a loop declares its own bound with
`limit` (1 to 100) on each `continue` operation: the continuation is admitted
while the count is below that limit, and notice turns in the same chain follow
the limit its latest continuation declared. A continuation the user asked for,
such as a Resume control, sets `user: true`: it is admitted whatever the count
and is the first turn of a new chain. A package states this as it may also
submit a turn in the user's name; it must not set it from `after_turn`. A
continuation also receives the session's waiting background results, held ones
included, as a user turn does.

A plain script tool handler may declare `"approval": true` to ask for the
ordinary tool approval before each call; without it a plain handler asks for
nothing, and a read-only handler never asks.

A refused continuation fails the transaction with `details: {kind:
"continuation", reason, count?, limit?}`. `reason` is `plan` (the session
is in Plan mode), `child`, `archived`, `worktree` (the worktree is gone),
`queue_paused`, `queued_input`, `running`, `interaction` (an approval or question
is pending), `limit` (with `count` and `limit`) or `stale` (the `after` turn is
no longer the latest completed turn). `queued_input`, `running` and
`interaction` mean another turn will end and call `after_turn` again; the others
need the user.

`host.turn.after_turn` names a handler called after every main task turn ends,
with `TurnEnd` (see `api.d.ts`): the turn's `sequence` in the session's order
(a queued turn starts as soon as its predecessor ends, so two callbacks can
overlap; ignore one older than a turn already handled), its `status` (`cancelled` is the user's
Stop, `interrupted` a turn the Node lost, `failed` carries `error`), whether it
was `automatic`, a `notice` turn or this package's own `continuation` and its
marker, `tool_calls` and per-name `tools` counts, `expired` approvals and
questions, `usage` for the turn and `session_usage` for the session (both
include delegated children), `background.running` and `background.pending`, and
the automatic `chain` `{count, limit}`. It is an ordinary callback with the
package's declared authority. `turn.initialize` receives the same `automatic`,
`notice` and `continuation` fields.

An explicit controller action can atomically resume an idle queue and admit a
fenced continuation with `prepareTransaction`. Put a `queue_pause` operation
before the state write and `continue`, with data `{session, expected_revision,
paused: false}`. The revision comes from `readConversation().page.queue.revision`
as an exact decimal string; queue operation receipts use the same representation.
This requires `conversation.control` and the captured session scope. Held input
or running work still rejects the continuation and rolls back all operations,
including the queue pause change. Automatic completion callbacks leave the queue
pause unchanged.

Core `spawn_agent`, `guide_agent` and `wait_agent` tools delegate in main sessions; packages may also bind `agents.delegate`, `agents.guide` and `agents.wait` with the `agents.delegate` action. Delegation is synchronous unless `background: true` is requested. Background admission returns the child session and turn; guidance takes that session and a message, and waiting observes its canonical outcome. Guidance and waiting are native tools, not callback flow steps. Only the running main session can control its children. Children keep their captured permissions and configuration, and approval remains required where that capture requires it. Main guidance can interrupt a wait at the settled tool-batch boundary without cancelling background children; explicit Stop cancels them. Initializers for delegation tools receive captured role summaries with `read_only` and `tools` booleans; `tools` reflects the frozen model override, or inheritance when no model is selected. These summaries grant no permissions and expose no provider configuration or credentials.

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

Successful tools default to `presentation: "summary"`: an activity row without
result expansion. Declare `details` for meaningful output, `content` for the
existing standard blocks, or `progress` for plans. `hidden` omits a successful
ordinary maintenance activity; approvals, questions, child agents and failures
retain their existing interactions. These declarations change no model result,
canonical tool output, permissions or execution scope. Captured explicit modes
remain part of historical calls; omitted modes use the current v1 default.

Core owns these presentation types and their renderers; any tool can select them
through its declaration. Rows show the localized tool name and captured input,
without appending result text. Summary-only results carry no output preview in
conversation synchronization; failed calls keep details available on demand.

Prefer `display.output` references for the actual body, paths, table or diff.
An explicitly declared output that resolves empty does not fall back to arbitrary
JSON. Conditional `preview.parts` and notices let a mixed tool keep queries brief
while exposing confirmed changes for mutations. A value part can explicitly set
`format: "json"` to inspect an object, array or scalar at its declared pointer;
the existing `when` condition still applies. JSON formatting is deferred until
details are opened and preserves business fields such as `id` and `node`.
Without this format, object and array values are not converted into preview text.
Ordinary undeclared MCP successes retain an explicit raw-result inspection action
without acquiring a normal result disclosure. `display.raw_result` is captured
only by Node and cannot be supplied by packages.

Node captures actual failed execution facts with the same canonical call/result
identity. These facts keep native errors and MCP `isError` responses inspectable
without interpreting a successful business field named `error` as a failure.

`display.input.summary` can use `format: "connection"` to show a saved connection's
public name. The execution Node captures names only from the connections admitted
to that turn; packages cannot supply the captured `names` map. Unknown or blank
names keep the original ID. Captured names remain stable after configuration changes.

`display.input.summary` can declare `result_path`, a JSON Pointer into the matched
native result. A nonblank string at that path replaces the displayed argument
after the result arrives, before applying the field's existing format. Missing,
blank or non-string values keep the original summary, including attachment IDs.
This display-only hint is captured with the call; it changes neither tool
arguments nor resource identity. Other input fields cannot declare it.

A tool declaration can set `presentation: "content"`. Return `sailry_content` in the native result or in MCP `structuredContent`; the Node preserves the latter under the result's `output` field. The declaration is captured by the execution Node with the tool call. Model arguments and output alone cannot select this presentation.

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

Content allows 1–16 blocks and at most 256 KiB of serialized JSON. Tables allow 1–64 columns, 1000 rows, 1024 bytes per column label and 16 KiB per cell. Diffs allow a 4096-byte path without control characters and 4096 lines. Tables and diffs accept `truncated: true` to disclose partial output. Unknown fields or kinds, unsupported versions, mismatched rows, missing or duplicate image references and failed results retain the original result for inspection. The shared Client validates the matched call and result, so controllers can consume the same content.

Desktop renders these blocks with the common Kit table, diff and authenticated image viewer. Native call state, approval, cancellation, folding and history order still belong to the conversation. Explicit raw inspection preserves the unchanged result when a standard block cannot be rendered. History remains readable after package removal and Node restart; no plugin script runs to render saved tool content. The built-in database query uses this same contract.

Plans retain `presentation: "progress"` and the `progress.update` operation.
Node validates the successful result as core `ConversationProgress`; Desktop
uses that typed message for the plan component and composer indicator, regardless
of the tool name. Questions and plan confirmation retain `ask_user` and the existing question state; external MCP servers can use form or URL elicitation. Content blocks cannot create approvals or questions or submit answers.

The runnable [tool-content MCP example](../plugins/examples/tool-content) returns a table, diff and image without a desktop entry.

## Boundaries

Headless `sailry/models.list({target:'node', kind:'ocr'})` reads the same Node model
registry as embedding discovery and installation. Package handlers use the native
`models.install` and `models.remove` operations for explicit Node model lifecycle
tools. Removal requires the original claimed command approval and keeps definitions
and settings. Model installation shares the existing bounded queue; inference
shares one CPU runtime. Plan turns cannot mutate models; callbacks cannot directly
dispatch model removal.

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

Plugin commands retain the existing declaration, exact installed-package, Node, worktree and session checks. File/Git/conversation operations use the existing scoped protocol through `prepareRequest`; HTTP uses the declared Node service; the SDK grants no raw process or OS access. UI and game rules stay in plugins. Persistence, credentials, model execution and durable receipts stay in the execution Node.

The `read_command` operation accepts `wait_ms` with a command `id`, capped at
30000 milliseconds. Zero or omission reads immediately. Without `until`, waiting
ends when new output arrives; with `until`, it waits for that substring in either
output preview. Exit and the deadline also end the wait. These reads use output
notifications and return a nondurable receipt before waiting. The same optional
fields are supported by the v1 `read_command` and `read_command_page` commands;
page reads preserve their selected stream, offset and limit. Listing commands
and reading an original tool-result reference do not accept wait options.

Node owns storage evolution. Startup validates schema updates on an isolated snapshot, saves a private backup and applies them transactionally. Invalid updates and unsupported generations preserve the existing database. See [storage ownership](../ARCHITECTURE.md#contract-and-storage-versions).

## Service connections

Declare `connectors` in the v1 extension. Each declaration has a stable `id`,
`display`, `accounts: "single"`, canonical HTTPS `origins`, bounded `fields`,
authentication `auth` methods, and explicit `inject` targets. Methods are
`secret`, `oauth2` (PKCE or device code), `application` (declared token exchange
and optional private registration flow), `mcp_oauth`, and `local_cli`.
Bundled OAuth clients contain only public client IDs. Secret fields never have
manifest defaults and are entered through the protected native form.

For HTTP, use `requestHttp({connector: "service", connection: optionalId, ...})`.
`connection` selects a captured instance; omitting it selects the package's single
connection. Node injects credentials only into declared headers at declared
origins. Request headers cannot override protected headers. Redirects are disabled.
A 401 may refresh credentials once; only GET, HEAD, or an explicitly
`idempotent: true` request may be sent again. An ordinary `credential` slot
remains available for non-connector services and cannot be combined with
`connector`. MCP injection uses a declared `server` plus `header` or `env`.
Actual server URLs and configuration stay private on the Node.

Headless flows can import `call`, `response`, `failure`, `text`, `id`, `query`
and `limit` from `sailry/http`. These pure helpers prepare declared HTTP steps,
decode outcomes and encode parameters. Packages retain service validation and
pagination; the Node executes returned calls through the scoped `http.request`
operation.

The `sailry/connectors` module exposes scoped `list`, `types`, and `status(id)`
public reads. Native surfaces prepare `login` and `cancel` drafts; headless
callbacks cannot authenticate. Connections use stable IDs and revision-checked
changes through durable admission. `ConnectionsChanged` supplies a snapshot and
cursor; caller-scoped login subscriptions expose transient authorization progress.
Replaying an uncertain command uses its original request ID.

OAuth scopes, client, flow, source or credential target changes suspend use for
consent. Public differences contain declaration data and MCP server names, never
private MCP configuration. Changed CLI or secret declarations do not require OAuth
consent, but their current target and revision fences still apply. A local CLI
source declares a fixed executable, argv, account JSON pointer and optional
status condition, minimal constant environment, timeout and output limit. Its
transient token is never persisted. Installation and updates disclose the CLI.

Application registration is an optional method-owned declaration using the
existing private `model_auth::Flow` runner and a token mapping that issues the
method's declared client fields. The package owns the registration script and
requests. Issued credentials are saved only against the captured connection
revision. Cancellation remains available after package changes; public snapshots,
JavaScript and history never contain registration secrets or private callback URLs.

## Inline conversation cards

Declare `cards` in the v1 extension. Each card has a unique `key`, a JavaScript
`entry` listed in `desktop.resources`, and a closed `props_schema`. Properties
are bounded strings, booleans, or integers, not nested data. Set
`additionalProperties: false`; strings require `maxLength` at most 4096.
Credentials, callback addresses, OAuth URLs, scripts and commands are not public
card properties. A card-only package does not need a workspace navigation entry.

An admitted host tool imports `card` from `sailry/cards` and `status` from
`sailry/connectors`, then loads
`connectionLabels` from its package's i18n resources:

```javascript
const connection = await status("owned-connector");
const props = {configured: connection?.state === "connected"};
return {sailry_content: {version: 1, blocks: [
  card("connection", props, {label: "Connect", locales: connectionLabels})
]}};
```

`status` reads only public connection metadata owned by the captured package. Headless tools cannot begin login. Node stamps the canonical
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
login. An optional `icon` asset path uses a compact icon button while retaining
`label` as its accessible name and tooltip. OAuth uses the existing Client
observer and Node MCP authorization engine, including original durable request
IDs and outcome inspection. URLs,
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
disconnection retain localized static fallback. Hidden conversations stop
subscriptions and clear private pending URLs. Scrolling within the current
conversation retains an already user-started pending login so its private callback
and reopening actions remain available; reopening does not begin another attempt.
Every service action rechecks canonical history, current package settings and scope
at Node admission. A login may advance
settings while immutable captured code remains available under the same digest.
Uncertain side effects retain their original request and require outcome
inspection, never automatic replay. These contracts do not establish real
Notion account or embedded sign-in acceptance.

## Package tests

Keep plugin behavior cases and assertions in the plugin repository. Go runtime
contract checks run the production moejs loader and SDK,
using in-memory bundles, captured context, optional invocation policy and exact
typed request/reply fixtures. Unmatched or unused replies fail the fixture.
Reports include the script result, observed requests and staged turn state.
These checks do not start a Node or substitute for local/remote integration.

The application test host is the test binary of `internal/node/plugins`; its
`TestMain` dispatches fixture requests when `SAILRY_PLUGIN_TEST_HOST` is set.
Build it explicitly with the environment and command in
[the plugin test instructions](../plugins/README.md#checks-and-catalog). The runner
sets that variable when given `--runtime-host`; it never builds the host implicitly.
Without an explicitly selected host, runtime cases are reported as skipped while
package JavaScript tests still run.
Core host tests use neutral fixtures and do not depend on bundled package names.

Package matrices can call `validateSchema(schema, values)` from the shared
JavaScript helper inside `runtimeTest`. This returns one validity flag per value
using the host's `--validate-schema` mode. Invalid schemas fail separately;
external references cannot read files or use the network. This checks the raw
schema, not a model provider's normalized declaration or enforcement.

`readConversationRuns(before = null, limit = 100)` returns bounded lifecycle
metadata for the captured session: `{session, revision, runs, next_before,
queue_revision, background}`. Each run contains only `{turn, sequence, status,
origin, error_code, recovered, context_overflow}`; `recovered` is present and
true when the run ended before this Node started, so no callback for it is
still to come, and `context_overflow` is present and true when the run failed
because its request exceeds the model's context window (its `error_code` is
`unavailable`, as for an outage).
`background` is `{running, pending}`: the session's background tasks still
running, and finished results still to be delivered to a turn; a result the
user stopped or the queue holds is in neither. A page without `background`
comes from a Node that does not state it. The page never loads message
bodies, admitted input or notices. Pages contain at most 100 runs, ordered by admission sequence within each page. Pass
`next_before` to read older runs. `readConversationRun(turn, revision = null)`
reads one visible run; a supplied structural history revision rejects stale
fork/rewind membership. Both methods require `conversation.read` and the captured
session. The page's `revision` and `queue_revision` are decimal strings; the latter
is the current queue control revision.
