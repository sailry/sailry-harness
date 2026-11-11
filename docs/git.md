# Git

The execution Node owns Git inspection, mutations, managed checkouts and output.
Controllers send v1 commands with captured worktree identity and observed HEAD,
branch and index revisions. The desktop Files and Git panels use core services;
package desktop surfaces do not supply these panels or the native Git badge.

`internal/node/git.Service` performs bounded reads through the pinned go-git v5
library. The required status trial selected the documented native status fallback:
its warm ratio and ignored-file parity failed acceptance. Native status disables
optional locks, fsmonitor, clean/process filters and renames. Concurrent reads of
a root share one request and a one-second cache. Mutations and relevant shared
watcher events invalidate that cache. Other mapped reads retain go-git.

Status includes per-file staged, unstaged and total line counts. One status
reads the HEAD tree and the index once and loads each side of an entry once for
its three counts. The optional
`GitEntry.modified_ms` field carries an observed modification time for the
activity badge across local and remote Client projections. Missing or deleted
paths omit the field. This additive v1 field implements the Go backend plan's
badge requirement; existing fields and absent-field fixture bytes stay intact.

Ahead and behind counts walk both histories newest first and stop at their
shared commits, as the reference's libgit2 call does, so the cost follows the
distance to the merge base and not the length of the history. Like Git, the walk
relies on commit dates.

History, committed files, blame and diffs retain source page and byte bounds.
Change inventory captures a fixed commit, range or worktree target. Every page
carries metadata and content revisions; replacement or external edits return a
revision conflict. Oversized, binary, symlink, submodule and non-UTF-8 changes
carry explicit skip reasons instead of incomplete review anchors. A worktree
inventory accepts a tracked file as unchanged without reading it when its size
and times (and on Unix its device and inode) still equal the stat data Git
recorded in an index entry older than the index itself; any other file is hashed. File reads use
the confined Core kernel. Index edits create a detached candidate, retain an
exclusive owned `index.lock`, compare source HEAD/index again and atomically
publish the candidate. One update evaluates ignore rules once for all of its
untracked paths. An added entry displaces the entries it cannot coexist with,
as `git add` does: those below its path and those that are one of its parent
directories. Unstaging restores the file HEAD records at the path and otherwise
removes the entry. Hunk staging additionally compares the source content
revision before applying its transform.

Commit, signing, hooks, remotes, transport, merge, rebase, cherry-pick and stash
operations use the Node's native Git executable and environment. They pass argv
directly, remove ambient `GIT_*` redirection and never invoke a shell. Native
children retain process-group or Windows Job ownership. Output is bounded,
credential-redacted and ephemeral; durable receipts do not contain hook output
or private remote URLs. A commit can succeed while its selected push or sync
fails; the result preserves the commit ID and separate follow-up failure.
Deleting a remote branch validates its remote name and requires a configured
remote like every other remote action. A failed native command is reported busy
only for Git's own lock failure (`Unable to create '….lock': File exists`), so a
conflict that names a file such as `Cargo.lock` stays a conflict.

Root owns shared Files/Repository watchers and publications. The Git filter
tracks index paths, Git ignore rules, linked common metadata, reference changes
and `.gitignore` invalidation. It excludes lock-file churn, object writes and
managed-child content. A changed path is matched against the tracked paths by
lookup, not by scanning them. No second OS watcher is created by the Git service.

## Deviations and validation

The accepted native hunk controls also act on manual working-tree edits that
have no conversation checkpoint. The original v1 protocol only supplies turn
hunk operations, so the authorized prerequisite adds `apply_git_hunk` and
`git_hunk_applied` in place at v1. The original 362 captured command fixtures
remain unchanged; the public inventory explicitly appends the new command.
Its strict payload captures worktree/path, working-tree scope, selected hunk,
full patch digest, complete file revision and expected HEAD/branch/index.
Nullable HEAD and branch fields must still be present on the wire.

`live/workspace_git_hunk.go` captures the exact displayed diff and rereads the
complete file before admission. Matching canonical turn review keeps the
existing `ApplyTurnHunk` path. Manual Stage transforms only the selected hunk in
the current index through `EditIndex`; Revert uses the existing confined
revision-checked file replacement. Keep remains local. Staged-only, binary,
truncated, conflicted, missing or stale sources are rejected. Repeated receipt
checks retain the same command and never replay a confirmed edit.

A manual hunk in All view is relative to HEAD. If its context overlaps an
already staged edit, staging reports a conflict when that HEAD-side context no
longer matches the current index. It preserves the index and working file; the
Unstaged view supplies the index-relative hunk. The service does not merge an
ambiguous partial staging operation or silently stage the complete file.

The live Git component retains captured commit and stash documents under its
existing Git tabs. Their native diffs are immutable and retain binary and partial
flags; command output stays literal in a read-only native code editor. Working
changes keep their existing revision-aware actions. Loading, plain directories,
nested repositories and unborn repositories have separate source states.
Initialization names nested repositories before admission. Checkout confirms the
shared worktree effect, resolves the existing tracking branch or asks for a local
name on collision, and offers Stash and switch after a real conflict. Uncertain
effects retain their original command and completion until Check result settles.

History uses actual Node cursor pages of 100 entries, retained previous pages,
real offset ranges and partial-content flags. Amending always uses the canonical
head message, independently of the displayed history page. Conflict paths disable
their staging controls and commits; active merge/rebase state supplies Continue
and Abort. Empty-staged commits choose tracked or all changes, and commits with a
push or sync follow-up choose an existing remote when no upstream is set. A
confirmed commit with failed follow-up reports the follow-up failure separately.

The plan maps named merges to native Git and forbids a second merge algorithm.
Unlike the reference's libgit2 in-memory named merge, a native merge conflict can
leave a conflicted index and `MERGE_HEAD`; the service exposes that real operation
state and never claims the worktree stayed unchanged. Stale worktree cleanup
uses a targeted native remove, preserving unrelated stale registrations.

The reference rejects unstaging a path that HEAD holds as a directory; this
service removes the staged file and leaves the directory's files to their own
paths. Unstaging a path below a file that HEAD holds is still reported as a
missing resource; unstaging that file restores it and displaces the path.

Default tests use temporary go-git repositories, injected native responses and
static argv. Integration-tagged probes execute native Git only in disposable
repositories; `TestLargeRepositoryIntegration` covers status, change inventory
and staging with 20,000 tracked files and branch listing over 150,000 commits. Windows Job behavior requires the Windows probe; local compilation
alone does not establish another operating system's behavior. Build and test
results belong to the coordinator's stage report.
