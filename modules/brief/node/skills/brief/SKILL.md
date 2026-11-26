---
name: brief
description: Create session-owned HTML documents, reports, diagrams, demos and interactive previews requested or accepted by the user, and small preview visuals that clearly help an answer. HTML does not authorize executing a plan.
metadata:
  display-name: "HTML documents"
  display-name.en: "HTML documents"
  display-name.zh-CN: "HTML 文档"
  display-name.zh-TW: "HTML 文件"
  display-name.ja: "HTML ドキュメント"
  display-name.ko: "HTML 문서"
  display-name.fr: "Documents HTML"
  display-name.de: "HTML-Dokumente"
  display-name.es: "Documentos HTML"
  display-name.pt-BR: "Documentos HTML"
  display-name.ru: "HTML-документы"
---

# HTML artifacts

## Choose the output target

If the user requests a specific worktree file path, create and verify that real
file with available file or command tools, then deliver it with `present_files`
as a file card with a sidebar preview. Without that tool, deliver its standalone
Markdown link. Do not substitute `write_brief` or create an additional
session artifact unless the user separately requests one. If file writing is
unavailable in this mode or tool catalog, explain the limitation rather than
claiming the requested file was saved.

`write_brief` creates only a session-owned artifact, not a worktree file. Its
returned card already delivers the artifact; do not invent a file path or add a
duplicate Markdown file link. A separately requested file copy must actually be
created and verified before linking it. Preserve the current work mode and
permissions; generating HTML does not grant filesystem access.

## Create a session artifact

For complex development, design or refactor plans, present the full ordinary plan
in the conversation first, then end with exactly one concise offer to create an
HTML version and wait for the user's choice. Do not repeat the offer for that
plan. Do not offer HTML for pure questions or simple changes, and never generate
a document or plan HTML version automatically without acceptance. An explicit
HTML or HTML-preview request authorizes generation directly. Choose content and
interactions for the user's actual request, not a prescribed plan or form. Use `write_brief`
in the main session; delegated agents can contribute analysis to their parent,
but cannot create or revise a brief. Plan mode permits this session-owned
document, without permitting project writes or execution.

Choose `kind=document` (the default) for structured documents, reports, diagrams
and optional forms. One template serves these document types. Write only the HTML fragment inside
`<main>`, without `<html>`, `<head>`, `<body>`, scripts, styles, event handlers,
external links or external images. The host supplies semantic theme colors,
fonts, local diagram libraries and interaction code. Do not generate JavaScript
or override the theme in document mode. Use a short title and card summary. After writing, report
the resulting document briefly; do not repeat its full HTML.

Before revising, call `read_brief` without a revision to inspect the current HTML,
then preserve its `kind` and pass its exact `id` and latest `revision` as `expected_revision`. A conflict
requires another read; do not overwrite or retry with a guessed revision. Preserve
the user's previous decisions and content unless the requested change replaces them.

## Proactive visuals

Without a request, you may create one small `kind=preview` visual when it
clearly helps the current answer more than text: a chart of the user's data, a
diagram of a flow or architecture, a UI mockup, or a short interactive
explainer. Keep the written answer complete without it and mention the visual
in one short sentence. Skip it for simple facts, routine changes, or when the
user prefers text. Use data from the conversation or a cited source; do not
fabricate measurements. Create at most one per answer. Documents, reports and
HTML versions of plans still need an explicit request or acceptance.

## Interactive previews

Use `kind=preview` when the request needs custom HTML/CSS/JavaScript, such as a
demo, dashboard, filterable checklist, local counters or clickable navigation.
Supply a complete self-contained HTML document. Inline CSS, scripts, event
handlers, DOM updates and in-memory state are supported in this mode; document
mode removes them. Keep the source within 512 KiB, without NUL characters.

Preview runs in an opaque-origin sandbox, with no Node, tool, credential,
filesystem or application form bridge. The wrapper restricts resource loading
and navigation with CSP, and denies popups and downloads. This is not an OS-level
zero-egress guarantee. Do not depend on CDNs, package imports,
external images, fetch, WebSockets or persistent browser storage. Use inline
assets, bounded data images and local JavaScript only. No dependency installation
or permission change is part of generating a preview. Local demo controls do
not send conversation messages or approve execution. Use `ask_user` separately
for actual user decisions or missing information.

Conversation cards may show a still image of a preview, and the side panel runs
it live. Leave the `html` and `body` backgrounds transparent so the host surface
shows through, and center the main element, for example with
`body{margin:0;min-height:100vh;display:grid;place-items:center}`. Keep visuals
modest: about 320 to 720 px wide and under 360 px tall suits a card; taller
pages are shown from the top. The host sets `color-scheme` to light or dark and
supplies these CSS variables: `--sailry-background`, `--sailry-foreground`,
`--sailry-muted`, `--sailry-muted-foreground`, `--sailry-primary`,
`--sailry-primary-foreground`, `--sailry-secondary`,
`--sailry-secondary-foreground`, `--sailry-accent`, `--sailry-border`,
`--sailry-success`, `--sailry-warning`, `--sailry-danger`, `--sailry-chart-1`
through `--sailry-chart-5`, `--sailry-font-family`, `--sailry-mono-font-family`,
`--sailry-font-size` and `--sailry-radius`. Always give a fallback, such as
`color: var(--sailry-foreground, CanvasText)`, because a host or export may omit
a variable.

For a copy button, copy selected text synchronously inside the user's click
handler with the browser's `document.execCommand('copy')` fallback. Do not read
the clipboard or copy automatically. The Async Clipboard API may be unavailable
in the opaque frame. Check the operation's result; on failure, keep the text
selectable for manual copying instead of showing a false success. Never route
authored scripts through a Node or privileged application bridge to copy text.
This browser fallback is platform-dependent; do not claim it has been tested
on the user's platform unless it actually has.

Preview exports retain the same sandbox and network restrictions in an
interactive HTML wrapper. They preserve the original source rather than capture
transient DOM edits or browser state. PNG export is not available for previews.
Structured documents retain their existing HTML and PNG export behavior.

## Document components

- Sections and contents: `<section data-title="Overview">...</section>`; use
  `h1` through `h4` for headings. Missing section headings are supplied from
  `data-title`. A lead paragraph uses `<p class="lead">`.
- Callouts: `<div class="callout info">`, `warn`, `risk` or `ok`; start with
  `<strong>Title</strong>` followed by the body text.
- Tables: ordinary `table`, `thead`, `tbody`, `tr`, `th`, `td` and `caption`.
- Steps: `<ol class="steps">`; file changes: `<ul class="files">` with a
  `code` path and concise explanation in each `li`.
- Status: `<span class="badge new">`, `changed`, `removed` or `risk`.
- Alternatives: `<div class="compare"><div class="option" data-title="A">...
  </div><div class="option" data-title="B">...</div></div>`.
- Details: `<details><summary>Details</summary>...</details>`.
- Diagrams: `<pre class="mermaid">flowchart LR ...</pre>`. Use `flowchart LR`
  for pipelines, `flowchart TD` for hierarchy and decisions, `sequenceDiagram`
  for interactions, `stateDiagram-v2` for lifecycles, and `gantt` for schedules.
  Keep individual diagrams under 30 nodes, quote Unicode labels, and avoid
  experimental types, configuration directives, frontmatter, links and HTML labels.
- Charts: `<pre class="chart">` containing the same closed JSON shape as chat
  charts. Types are `line`, `bar`, `area`, `pie`, `radar`, `candlestick`, `sankey`.
  Data comes from the user's data or a cited source; do not fabricate measurements.
- Formulas: `<span class="math-inline">x^2</span>` for inline TeX and
  `<pre class="math">...</pre>` for an independent formula. Write the TeX source
  without dollar delimiters; the local renderer supplies mathematical layout.
  Use `\text{}` for words inside formulas, and write plain prose definitions as
  text rather than math.

```html
<pre class="chart">{"type":"bar","title":"Sales","data":[{"label":"Jan","value":120},{"label":"Feb","value":150}]}</pre>
```

For line, bar, area, pie and radar, points contain only `label` and numeric `value`.
Candlestick points contain only `label`, `open`, `high`, `low`, `close`, with
valid high/low bounds. Sankey uses `nodes:[{"id":"a","label":"A"}]` and
`links:[{"source":"a","target":"b","value":1}]`, and must be acyclic.
Optional `title` is a string. Do not pass arbitrary ECharts options, callbacks,
colors or fields. Each chart or diagram is limited to 64 KiB. A document body
is limited to 512 KiB after cleaning.

## Questions

Forms are optional and only supported in `kind=document`; a preview's form
submits nothing. When the requested structured document benefits from collecting
unresolved decisions or information, it may include a `brief-form`. Do not append
one to every document. Use radio choices for one answer,
checkboxes for multiple answers, and a textarea for additional context. Include
only decisions that remain open; preserve confirmed choices when revising.

Use a bounded declaration rather than executable controls. Form and field names
must start with a Latin letter and contain only letters, digits, `_`, `.` or `-`,
up to 64 characters. Use unique form and field names. Each form has at most
32 fieldsets and at most 64 controls per fieldset. Each control's `name` must
match its containing fieldset's `data-name`. A fieldset may carry `data-required`.
Radio and checkbox values and labels are at most 512 bytes; free text is at most
4,096 bytes. Total submitted answers are limited to 16 KiB.

```html
<form class="brief-form" data-name="decisions">
  <fieldset data-name="approach" data-required>
    <legend>Preferred approach</legend>
    <label><input type="radio" name="approach" value="a"> Approach A</label>
    <label><input type="radio" name="approach" value="b"> Approach B</label>
  </fieldset>
  <fieldset data-name="note">
    <legend>Additional context</legend><textarea name="note"></textarea>
  </fieldset>
  <button type="submit">Submit</button>
</form>
```

The user answers in the side panel. Answers are ordinary user messages that
start with a reply line naming the document, its version and the form. While
a turn is active, they enter the normal message queue. Only the latest version
of the document in its own conversation can submit. Answers do not approve or
start execution, answer another pending question, or change the work mode. When
answers arrive, read the latest revision and revise the document to reflect them
unless the user asks otherwise. Use `ask_user` separately when the actual plan review requires it.

## Suggested structure

Choose sections according to the user's needs; these are recommendations.

- Development design: goals and conclusion, current state, architecture,
  detailed flow/data design, changes, implementation steps, validation, risks
  and rollback, unresolved decisions.
- PRD: background and goals, users and scenarios, prioritized requirements,
  user flow, nonfunctional requirements, milestones, unresolved decisions.
- Technical review: conclusion, architecture and boundaries, decisions and
  alternatives, risks, remaining questions.
- Incident report: impact, timeline, root cause, response and recovery,
  improvements with owners and dates.
