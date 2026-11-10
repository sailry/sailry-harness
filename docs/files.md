# Files

Workspace file services live in `internal/node/files`; retained filesystem
capabilities and native Trash live in `internal/core`. The Node captures registered
worktree roots and access before releasing SQL leases. Every physical read,
stream and mutation runs outside SQL transactions and publication locks.

Directory pages sort directories first and retain at most 500 entries plus one
lookahead. Cursors bind the observed directory revision. Whole text reads return
at most 128 KiB and expose a full BLAKE3 revision only for complete content. Line
reads, file discovery and ignored-tree traversal reuse the core file owner.
Search supports literal and RE2 queries, content, files and counts, context,
pagination, strict text decoding and the vendored ripgrep default file types.
Unsupported or changing files produce bounded diagnostics and honest partial
results. Incomplete scans never resolve a later content offset.

Text edits resolve all exact matches before publication, preserve CRLF boundaries
and reject ambiguity or overlap. Change sets validate all targets before the
first effect, require one approval, retain exact prepared content and stop on the
first failed or uncertain effect. Already applied files remain applied; later
files are marked unattempted. Patches use the same prepared-change path and
require an observed full revision before changing an existing file. Notebook
creation assigns deterministic cell IDs; edits keep original index meanings,
preserve unknown fields and clear edited code outputs.

Entry copies retain confined source and target directories, reject links and
special files, enforce depth and entry limits, skip Git metadata on copies and
preserve it on whole-tree moves. Same-filesystem moves use native rename.
Cross-filesystem moves verify source and target identities, metadata, complete
children and BLAKE3 content before passing the source to Trash. Host picker
actions use absolute execution-host paths, protect home ancestors and report
per-entry conflict, replacement and cut outcomes.

One `Transfers` instance owns raw authenticated streams. A stream is caller-bound
and opens once. Uploads verify exact size and BLAKE3, sync protected staging,
receive write-half EOF and acknowledge readiness before a durable finish
publishes a retained target. Downloads retain their verified open source and
Client verifies received bytes. Preparation, opening and idle limits are 8, 60
and 30 seconds; injected clocks support deterministic tests. Cancellation cannot
undo publication. Shutdown drains active workers before collecting orphaned
attachment files.

Attachment IDs determine storage names; display names never become paths.
SQL binds immutable ordered references to turns. Reads from agent execution
require visible history membership, including joined guidance, and exclude
pending or active unrelated origins. Discard requires the staging caller and
refuses any attachment already bound to a conversation. Admitted commands receive
verified read-only copies under one private scratch directory, named only by
attachment IDs. `SAILRY_ATTACHMENTS` names that directory. Process cleanup releases
only its retained scratch identity and preserves the immutable attachment source.

Checkpoints retain the exact prepared before and after content before an approved
mutation dispatches. Their receipts come from the durable request ledger, not
filesystem inference. Turn diffs join adjacent confirmed versions only when their
content matches, preserving gaps caused by external edits. Hunk reject uses exact
whole-line context and a current revision; hunk stage uses the Git owner's one
index writer with captured working, index and HEAD revisions. Restore refuses
active turns and absent outcomes, and verifies the written after revision.

The accepted native Files layout, view-model fields and Actions remain the UI
contract in [panel.md](panel.md). The coordinating Node owns shared native
Files/Repository watches, scope authorization, durable admission, approval
binding, publication ordering and live adapters. This package does not mount a
plugin-specific Files desktop surface.

Conversation inventory pages derive from canonical conversation events. They
retain visible fork membership, exact inline and tool image descriptors, captured
worktree roots, explicit attachment and file references, standalone Markdown
deliveries and published file or document content. Ordinary workspace-write
receipts do not become output files. Each read scans at most 256 events and
4 MiB, with 1 to 50 groups and an exclusive session-bound sequence cursor.
The inventory adds no filesystem scan, business table or second file index.

Expanded tool details derive a successful single-file edit from its captured
checkpoint, matching the reference detail reader. Later disk changes cannot
alter that display. Change sets and notebooks retain their declared result;
failed calls never acquire a successful diff. Unified file headers and hunk
metadata do not contribute to the conversation's added/deleted line counts.
