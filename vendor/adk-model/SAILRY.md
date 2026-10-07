# Gemini instruction routing patch

Source: `adk-model` from `dux-web/adk-rust` revision
`b3e360bbf49ca39d49cccba182c0f7cb4d251970`, under its retained Apache-2.0 license.
The existing provider patches are described in the retained Cargo manifest.

The default Gemini generateContent transport now routes system-role text through
the pinned `adk-gemini` builder's `with_system_instruction` API. It preserves text
and ordering across system messages and parts, joining them with newline
separators instead of discarding them or adding them to the user transcript.
Both blocking and streaming requests use the same builder. User messages, model
history, tool responses and generation options retain their existing conversion.

Remove this patch when the pinned upstream adapter preserves system instructions
through the same request boundary.
