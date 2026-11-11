# Model capabilities and media routing

Node owns media routing in `internal/node/media_tools.go`, the assignments in
`internal/node/store/media.go` and the provider calls in
`internal/node/agent/model/media.go`. The bundled `media` package declares the
agent tools; Node binds them. Preview data never calls a media model.

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
bytes are never logged; provider error text is bounded and redacted.

## Tools

The `media` package declares `inspect_image`, `generate_image` and
`generate_video`. A declared tool exists for a turn only when its kind routed.
Inspection is read-only. Generation needs approval and is refused in plan mode.

Generation checks the output path before the provider call: it is
worktree-relative, ends in `.png` or `.mp4` and names no existing file. The
confined file service writes the result without replacing a file. The tool
result carries `path`, `media_type`, `size`, `provider`, `model` and a video
`job`. A generated image is also stored as a tool-result attachment, so the
conversation shows it; it is never sent back to the model. A video job that the
provider accepted but whose result was not saved answers `outcome_unknown` and
names the job.

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

## Tests

`TestMediaDrivers` covers the channel table and request shapes against local
fakes. `TestMediaTools` covers routing, a real turn that generates and inspects
an image, output confinement, plan mode and speech. The store fixture
`provider_capability_tags_and_media_assignments` covers tags surviving a catalog
refresh and assignment validation. No real provider is called.

## Deviations

| Deviation | Reason |
| --- | --- |
| Speech synthesis records no session usage | The command has no session or turn to attribute it to |
| `use_media` is not a routed controller command | It only identifies the approval; the engine tool runs the operation |
| Account sign-in variants of the hosted image tool are untested | Tests use API-key fakes only |
