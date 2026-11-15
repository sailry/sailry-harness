The model capability catalog is copied unchanged from rig-core 0.46.0,
`src/catalog/models.json`. It is used only for the reviewed source request
parameter policy, including adaptive thinking and dated snapshot lookup.
See ../LICENSE.rig. No runtime or build access to a source checkout is needed.

`catalog.go` applies two reviewed lookup corrections without changing this source:
Claude Haiku 5.5 supports adaptive, context-bound thinking, and the dotted `5.5`
spellings of Haiku, Sonnet and Opus share their canonical `5-5` capabilities. These corrections
also apply after dated snapshot lookup. They do not rewrite the requested model
ID or infer capabilities for other model versions or providers.

Haiku's adaptive thinking requirement is documented in the
[official migration guide](https://platform.claude.com/docs/en/models/haiku-5-5/migration-guide).
Pi applies a corresponding reviewed capability supplement in its
[Claude family rules](https://github.com/earendil-works/pi/blob/04fd0897e4df737cd8422bc99085d6953da804ec/packages/ai/catalog/families/anthropic.ts).

The request policy also reads the catalog's `thinking_off` value. Sonnet 5.5 uses
`between_tools` instead of `disabled`, as documented in the
[official thinking reference](https://platform.claude.com/docs/en/build-with-claude/thinking-troubleshooting).
