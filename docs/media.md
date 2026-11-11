# Model capabilities and media routing

Node owns media routing in `internal/node/media_tools.go`, the assignments in
`internal/node/store/media.go` and the provider calls in
`internal/node/agent/model/media.go`. The three agent tools are built in: their
specifications are `internal/node/agent/operations/media.json` and Node binds
them for each turn. No plugin package takes part. Preview data never calls a
media model.

## Capability tags

Each configured model carries `vision`, `tools`, `reasoning` and `generates`
(`image`, `image_tool`, `video`, `speech`). Presets, the reference catalog and
discovery supply the defaults. A model saved with `custom_capabilities: true`
keeps its saved tags through catalog refresh and discovery; without the marker
the tags follow the catalog again.

Automatic defaults beyond the catalog: a catalog entry whose output is audio
without text gains `speech`; a Responses model with tools and vision on the
official OpenAI endpoint or a ChatGPT sign-in gains `image_tool`.

## Assignments

Media settings hold one optional binding for each of `vision`, `image`, `video`
and `speech`. An absent binding follows the conversation model.

| Command | Result |
| --- | --- |
| `read_media_settings` | `media_settings {revision, bindings, unavailable?}` |
| `list_media_models` | `media_models [{provider, provider_name, model, kinds}]` |
| `save_media_settings {revision, bindings}` | The stored settings and a `media_settings_changed` event |

`list_media_models` lists only enabled providers' models whose tags and channel
support the kind. A save validates only the bindings it changes: an unsupported
model is `invalid_request`, a missing provider or model is `not_configured`, a
stale revision is `revision_conflict`. `unavailable` is computed on every read
and names the bound kinds whose model is disabled, removed or no longer tagged;
it is ignored on save.

## Routing

The assignments are captured with the turn when it is admitted
(`agent_runs.media`), and a delegated child turn captures its own.

1. An assigned model is always used.
2. Otherwise the conversation model is used when it has the capability:
   `image_tool` runs the hosted image tool through Responses, `image` the images
   endpoint.
3. Otherwise the tool is not offered.

An assigned model that cannot run offers no tool and is reported in
`unavailable`. No other model replaces it.

## Channels

| Kind | Channel | Providers |
| --- | --- | --- |
| Vision | Normal chat request with one image part | API key, plugin adapter with vision, ChatGPT sign-in on Responses, Copilot, Claude sign-in on Anthropic |
| Image | `POST /images/generations` | API key on Chat Completions or Responses |
| Image | Gemini `:generateContent` with image modality | API key on Gemini |
| Image | Hosted `image_generation` tool on Responses | API key, ChatGPT OAuth, Copilot |
| Video | `POST /videos`, poll `GET /videos/{id}`, `GET /videos/{id}/content` | API key on Chat Completions or Responses |
| Speech | `POST /audio/speech` | API key on Chat Completions or Responses |

Calls reuse the provider's authorization, credential renewal, proxy and headers.
Responses are bounded: images 64 MiB, video 512 MiB, speech 8 MiB. Deadlines are
240 seconds for images, 540 seconds for video and 120 seconds for speech and
inspection idle time. Redirects are refused, an image URL must be HTTPS or the
provider's own origin, and non-PNG images are re-encoded to PNG. Prompts and
bytes are never logged; provider error text is redacted, then bounded, so the
bound never leaves part of a credential.

## Tools

`inspect_image`, `generate_image` and `generate_video` are built-in tools under
those plain names. A tool exists for a turn only when its kind routed.
Inspection is read-only. Generation needs approval and is refused in plan mode;
the approval is bound to the tool and operation without a plugin owner, and
approving for the session is remembered. Labels and input and output display
come from the engine's core tool presentation.

`inspect_image` takes `prompt` and exactly one of `path` or `attachment`.
Generation takes `prompt` and an optional plain `file_name`; the result is
saved as `assets/generated/<file_name>`, named by a new identifier when the
name is absent.

Generation checks the output path before the provider call: it is
worktree-relative, ends in `.png` or `.mp4` and names no existing file. The
confined file service writes the result without replacing a file. The tool
result carries `path`, `media_type`, `size`, `provider`, `model` and a video
`job`. A generated image is also stored as a tool-result attachment, so the
conversation shows it; it is never sent back to the model. A video job that the
provider accepted but whose result was not saved answers `outcome_unknown` and
names the job.

### Retired media package

The bundled `media` package that declared these tools and a settings page is
retired (`plugins.RetiredBundled`). At start Node removes a bundled-origin
installation of it: the removal is recorded, the package is not installed
again, and its captured versions stay readable for sent turns. A package from
another source that declares a handler operation `media.inspect`, `media.image`
or `media.video` still validates, but the declaration binds no tool. The plugin
actions `media.settings.read` and `media.settings.write` still authorize the
assignment commands. Records written under the package's tool aliases keep the
presentation stored with them, so earlier conversations display unchanged.

Each provider call records usage under the provider and model that ran it,
marked with its media kind and hidden from the transcript and the model.

## Speech

`synthesize_speech {text, voice?}` answers
`speech {media_type, data_base64, provider, model}` for Desktop read-aloud;
`Client.SynthesizeSpeech` returns the media type and decoded bytes. Text holds
1–4096 characters, the voice defaults to `alloy` and the audio is MP3. Speech
has no conversation model to follow: an absent or unavailable assignment is
`not_configured`. The command is controller-only, changes no state and is not
an agent tool.

## Desktop

Desktop edits tags and assignments and reads aloud; the Node owns both
contracts and all routing.

| Control | Where | Owner |
| --- | --- | --- |
| Capability boxes | Settings › Providers › provider editor › model form, above Reasoning levels | `settings/providers.go` `capabilityForm` |
| Capability marks | Provider rows' model chips; composer model picker rows | `settings/providers.go` `capabilityMarks`, `composer/models.go` `modelRow` |
| Capability filter | Composer model picker's rail, under the families | `composer/models.go` `modelPicker` |
| Media models | Settings › Media models, in the AI group after Knowledge | `settings/media.go` `mediaModels`, `live/settings_media.go` |
| Read aloud | The turn's existing Read aloud / Stop reading action | `reader`, bound in `cmd/sailry-desktop` |

The boxes are grouped Understands: Image; Can: Tools, Reasoning; Generates:
Image, Video, Speech. They show the model's tags; the first change marks the
model `custom_capabilities` and the provider save then sends the boxes as
its tags. A model whose boxes were not changed is saved as before, so the
Node keeps tagging it. The one Generates Image box stands for both image
tags: ticking it restores `image_tool` when the model had it as the editor
opened, and sets `image` otherwise. While a model's tags are the user's, the
Capabilities heading carries Reset to model data (`provider_cap_reset`): it
clears `custom_capabilities`, so the save leaves the tags to the Node's
catalog again, and shows the tags the model had as the editor opened when
those were the catalog's. A model that was already the user's keeps its
boxes until the save returns the catalog's tags.

Marks are the existing `Eye` (vision), `Image`, `Volume` (speech) icons and
`reicon-video-play` (video), each with a tooltip. The picker's rail shows a
filter toggle for each capability at least one offered model has; choosing
one again clears it.

`Runtime.followMedia` reads `read_media_settings` and `list_media_models`
on the local Node whenever the snapshot's media revision or a provider's
revision changes. A change that arrives during a read is read for when that
read ends, and an answer older than the revision shown, as one overtaken by
a save, is dropped. The section shows its title and description over one card
without a group heading. Each of the four rows (Vision, Image, Video, Speech) is a
Kit select of the candidates for that kind under Follow conversation model,
which is an absent binding. A choice sends `save_media_settings` with all
four bindings at the revision read. Failure, conflict and an uncertain
outcome are separate toasts and keep the choice as the row's draft while the
Node's settings are read again; success shows no toast. A saved binding
listed in `unavailable` shows the provider rows' warning dot with Model
unavailable, and a bound model that is no longer a candidate stays in its
row's options.

Read aloud asks `Client.SynthesizeSpeech(ctx, text, nil)` for the spoken
text in parts of at most 4096 characters cut at sentence ends
(`reader.Chunks`), up to two parts ahead of the one playing, and plays each
MP3 in turn through MyGo's `audio` package and its pure-Go `audio/mp3`
decoder. `not_configured`, any other synthesis failure, a clip that cannot
be decoded, one whose decoding fails before its end (`reader/playback.go`
keeps the decoder's error, which the player itself ends on silently) and a
missing audio output all hand the unread remainder to the
OS speech without a toast; failures other than `not_configured` are logged.
The existing failure toast shows only when the OS speech cannot start
either. Stop, another turn's Read aloud and closing the service cancel the
request under way and the clip playing.

## Tests

`TestMediaSettings` (`internal/desktop/live`) drives the Media models section
against a fake Node: each row's options, the bindings and revision a choice
saves, the draft and toast after conflict, failure and an uncertain outcome,
the unavailable mark, and the tags a provider save sends.
`TestProviderEditor` (`internal/desktop/settings`) covers the boxes showing
tags, the Image rule and the draft handed to the save.
`TestSettingsSectionsOpen` (`internal/desktop/shell`) checks that the section
has one navigation entry and four rows and that Models has none. `TestReadAloud`
(`internal/desktop/reader`) covers parts, order, every fallback and Stop
with a fake voice, player and OS speech. Its one clip for the real player
is sixteen silent frames and a frame that fails to decode, heard as nothing
and played into MyGo's null device where the machine has no output.

`TestMediaDrivers` covers the channel table and request shapes against local
fakes. `TestMediaTools` covers a profile with the retired package starting
without it, routing, a real turn in which the model is offered and calls the
built-in tools with no media package installed, no tool for a model without
the capability, output confinement, the remembered session approval, plan mode
and speech. The store fixture
`provider_capability_tags_and_media_assignments` covers tags surviving a catalog
refresh and assignment validation. No real provider is called.

## Deviations

| Deviation | Reason |
| --- | --- |
| Speech synthesis records no session usage | The command has no session or turn to attribute it to |
| `use_media` is not a routed controller command | It only identifies the approval; the engine tool runs the operation |
| Account sign-in variants of the hosted image tool are untested | Tests use API-key fakes only |
| The capability filter is in the composer picker's rail, not on the Providers page | The picker already filters by family there; the Providers page keeps its search and provider-type filter unchanged |
| The retired package's `imagegen` skill has no core counterpart | Core has no built-in skills; the tool description carries the rule |
| Protocol keeps the `media.inspect`, `media.image` and `media.video` action and operation values | Stored plugin records, approvals and captures decode strictly |
| A hosted image tool unticked and saved comes back as `image` when ticked later | The editor only knows the hosted tool from the tags it opened with |
| Media models and the read-aloud voice are the local Node's | The page's other Node settings are too; read aloud is a controller effect |
| MP3 playback through real audio hardware is not covered by tests | Tests replace the player; the decoder and output are MyGo's |
