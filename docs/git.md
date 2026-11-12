# Git

The execution Node owns Git inspection, mutations, managed checkouts and output.
Controllers send v1 commands with captured worktree identity and observed HEAD,
branch and index revisions. The desktop Files and Git panels use core services;
package desktop surfaces do not supply these panels or the native Git badge.

`internal/node/git.Service` performs bounded reads through the pinned go-git v5
library. The required status trial selected the documented native status fallback:
its warm ratio and ignored-file parity failed acceptance. Native status disables
optional locks, fsmonitor and renames. Concurrent reads of
a root share one request and a one-second cache. Mutations and relevant shared
watcher events invalidate that cache. Other mapped reads retain go-git.

Status includes per-file staged, unstaged and total line counts. One status
reads the HEAD tree and the index once and loads each side of an entry once for
its three counts. An entry changed on one side only is compared once. A side
over 1 MiB is neither read nor hashed and its counts are marked `truncated`,
as the reference marks them; binary content counts no lines. The unstaged side
is the content Git would store ([Stored content](#stored-content)). Counting takes at
most one second of the five-second status deadline: entries past it, and a
comparison cut short, are marked `truncated` and the status still answers. The
reference has no such share because libgit2's line diff is fast enough; the
go-git line diff is not, and ran without a usable limit, so several regenerated
lock files failed every status with a deadline. Its other callers (diff, hunk,
blame, history, change review) now stop refining at the five-second read
deadline and return the coarser, still correct diff. The optional
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
recorded in an index entry older than the index itself; any other file is
compared by the blob Git would store for it ([Stored content](#stored-content)). File reads use
the confined Core kernel.

Staging whole paths runs native `git add --all` on exactly those paths after
comparing HEAD and the index revision, so the index holds what Git stores: the
output of the path's clean filter (LFS) and of its line-ending conversion. The
reference, and this service before, wrote the worktree bytes as the blob, which
committed an LFS file's content instead of its pointer and CRLF where the
repository stores LF, while Commit all (`git add --all`) was right. Three rules
of the earlier operation stay: an ignored untracked path is refused, a
directory is not a path to stage, and a path that is neither on disk nor in the
index is skipped. One update evaluates ignore rules once for all of its
untracked paths. Staging a conflicted path records its resolution, as
`git add` does anywhere; the reference refuses every index update while a
conflict exists, which left Abort as the panel's only way out of a conflicted
merge. A directory the index records as a submodule is staged too: `git add`
records the commit the submodule has checked out, where the reference refuses
every directory.

Unstaging and hunk staging create a detached candidate, retain an exclusive
owned `index.lock`, compare source HEAD/index again and atomically publish the
candidate. An added entry displaces the entries it cannot coexist with, as
`git add` does: those below its path and those that are one of its parent
directories. Unstaging restores the file HEAD records at the path and otherwise
removes the entry; it involves no filter. Hunk staging additionally compares
the source content revision before applying its transform, and stores the
result through `git hash-object -w --path`, which applies the path's
line-ending conversion. A path with a `filter` attribute (LFS) has no hunk
actions: its index content is the driver's output, not lines of the file, so
staging and reverting a hunk are refused with a message that names the driver.
The hunks of a file whose line endings are converted are lines of its stored
form. Staging one stores those lines. Reverting one writes the restored lines
with the file's line ending, and is refused for a file that mixes line endings,
whose bytes the stored form does not determine. A file converted in another way
(`ident`, `working-tree-encoding`) has no hunk actions.
A staged hunk's index entry records no file size, as after
`git update-index --cacheinfo`, so Git compares the file's content; the size of
the staged text, recorded before, made `git status` list a converted file whose
last hunk was staged.

Commit, signing, hooks, remotes, transport, merge, rebase, cherry-pick and stash
operations use the Node's native Git executable and environment. They pass argv
directly, remove ambient `GIT_*` redirection and never invoke a shell. Native
children retain process-group or Windows Job ownership. Output is bounded,
credential-redacted and ephemeral; durable receipts do not contain hook output
or private remote URLs. A commit can succeed while its selected push or sync
fails; the result preserves the commit ID and separate follow-up failure.
Local native mutations keep the reference's two-minute limit, after which the
outcome is unknown. Transfers with a remote (clone, fetch, pull, push, sync, the
pushes that delete a remote tag or branch, and a commit's push or sync) have no
total deadline, unlike the reference, whose command-line transfers share the
two minutes: their duration follows the repository and the connection. They run
with `--progress` and are stopped after two minutes without output, which is
then an unknown outcome. The capture keeps what a terminal would show of the
progress, so Git's final reason is not pushed out of the bounded output. A hook
that stays silent for two minutes during a push is stopped the same way.
A pull is two native commands, `git fetch` and then `git pull`, so that the
transfer does not hold the worktree's effect order
([Projects](projects.md)); the reference runs `git pull` alone under its
Node-wide lock.
A native command that is stopped (its deadline, the silence limit, a cancelled
request or Node shutdown) is first asked to end with a termination signal to
its process group and killed two seconds later if it has not ended. Git removes
its lock files (`index.lock`, the reference locks) when asked and cannot when
killed, which is what the reference's immediate kill does: the stopped command
then leaves a lock that fails every later one as busy. Windows has no such
request for a process without a console, so its job is terminated at once and
can leave the lock. A lock left by a crash of the machine or of the Node, or by
the user's own Git, is never removed here, because another Git process may own
it; operations report busy until `.git/index.lock` is deleted by hand.

The agent's own `git` in a command and the panel are ordered against each other
by Git's lock files only, not by the worktree's effect order: the loser fails
as busy and nothing is written twice. The detached index writer used for
unstaging and staged hunks takes the same `index.lock`.

An unfinished revert (`REVERT_HEAD`) is reported as the operation `revert`, and
Continue and Abort run `git revert --continue` and `--abort`. The reference
reports it as a rebase and then refuses both, leaving no way out in the panel.
The panel starts no revert itself; the state comes from a command.

### Stored content

Status and the worktree change inventory follow Git's own rule for a worktree
file. A file whose stat data matches its index entry is unchanged and is not
read. Any other file is compared by the id of the content Git would store for
it: the output of the path's conversion (`core.autocrlf`, the `text` and `eol`
attributes, `ident`, `working-tree-encoding` and a `filter.<name>` driver such
as LFS). An LFS file that was only touched, or a CRLF file of a repository that
stores LF, is therefore unchanged. The reference disables the filter programs
for status and compares worktree bytes everywhere, which shows such a file as
modified, with every line changed.

Git performs every conversion and nothing is written: no reader stores an
object in the repository or anywhere else. Status is native `git status` with
the configured filters left on, so Git runs a filter exactly for the paths it
reads again. Like `git status`, it lists a file whose size differs from the
size the index recorded without comparing content; such a file can have an
empty diff. The inventory asks `git hash-object --stdin-paths`, without `-w`,
for the ids of the files it has to read again: one process per 1,000 paths,
never one per file. Equal ids mean unchanged. It compares content where
`git status` trusts a changed size, so a file stored unchanged is absent from
it. A changed-stat file over 32 MiB is listed without comparison.

Diffs and line counts have two cases.

- Line endings. A file whose bytes with CRLF replaced by LF hash to Git's id is
  compared as that text, converted in memory. An edit of a CRLF file diffs and
  counts its edited lines only.
- Filter drivers. A path whose `filter` attribute names a configured driver
  (`git check-attr filter`, which runs no program) is not compared as text.
  Its diff is the binary card with the file's size (`GitDiff.size`, also set
  for every other binary diff: the new side, or the old side of a deletion),
  its line counts are zero like a binary file's, and the inventory skips it as
  `binary`. This is the same whether or not the filter program can run, and
  neither the diff nor the counts run it. A file converted in another way
  (`ident`, `working-tree-encoding`) is shown the same way. A repository
  without filter drivers is not asked for attributes.

Cost and bounds. Nothing runs for a file whose stat data matches. Each status
runs the filter of every listed-or-touched filtered file once, inside
`git status`, because the status takes no lock and so cannot record the
refreshed stat data as an interactive `git status` does; `git add` or any
command that refreshes the index ends that. A status with unstaged changes
starts at most two more processes, for the attributes and the ids of at most
500 files of at most 1 MiB, inside the one-second counting share. Each process
has the five-second read deadline and stops with its request; when one fails or
is stopped, its batch is compared by worktree bytes. When `git status` itself
fails with filters on, as it does for a `required` filter whose program is
missing, the status is read once more with the configured filter programs
disabled, so the panel still answers: the file is listed as modified, without
an error or a notice.

Measured with a counting `tr` filter and `text=auto eol=lf` in a temporary
repository (`TestStoredContentIntegration`), which also asserts that status,
the inventory and the diff leave the repository's objects unchanged. Real Git
LFS was not run.

A remote's name may hold a slash, so deleting a remote branch and Pull from
split `remote/branch` at the longest configured remote that begins the name, as
the reference does; a name no configured remote begins is refused as not
configured. A path below what HEAD records as a file or a submodule is a path
HEAD does not hold, for unstaging, discarding and the HEAD side of a diff. A failed native command is reported busy
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
head message, independently of the displayed history page. A conflicted path
shows as an unstaged change with the letter U and blocks commits. Stage on that
file's own row records its resolution; Stage all, a folder and a group skip
conflicted paths, and a conflicted path has no Unstage. Active merge, rebase or
cherry-pick state supplies Continue, once no conflict remains, and Abort.
Before, a conflicted path had no staging control at all and, having neither a
staged nor an unstaged kind, no row in the staged and unstaged groups, so
Continue could only be reached through a terminal. Empty-staged commits choose tracked or all changes, and commits with a
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

`discard` restores paths from HEAD and, as the reference, refuses the whole
selection when HEAD does not hold one of them: restoring would delete a staged
new file that exists nowhere else. Unlike the reference's panel, which sends
`discard` for any selection and lets the Node refuse it, Discard Changes on a
selection with a new file (untracked or a staged addition) sends `discard_all`
and shows its confirmation: the new files are unstaged and moved to Trash. Undo
in the combined diff sends `discard_all`, as the reference.

The page's chord is a key sequence, as the reference's: the key pressed after
Ctrl+G completes it (Ctrl+G fetches, Down pulls, Shift+Down pulls with rebase)
or drops it, so a later Down does not pull. Down is asked for only while the
chord waits, which leaves it to the lists otherwise; while the change list or
the commit field has the focus they take Down first and the chord drops.
Deleting a stash names it by its message in the confirmation and drops it by
its ID.

The worktree manager lists every worktree `list_worktrees` reports for the
project: the main one, linked ones, a locked marker and stale registrations
whose directory is gone, with the current worktree checked. Choosing an
available worktree offers Open worktree (registers it, then switches the unsent
draft or moves the idle conversation), Copy path and Remove worktree; choosing a
stale one confirms a targeted `prune_worktree`. Removal names the path and its
consequence once and sends `remove_worktree` with the listed head and branch; the
Node, as Git does, refuses a dirty, locked or busy worktree. Removal is not
offered for the current, main, locked or detached worktree, and pruning is per
entry. New worktree asks for a starting point (branch, tag or commit, resolved by
`resolve_git_revision`), a new branch and a path. A pruned worktree's Node
record is not deleted; no command exists for that.

Git and worktree failures are named by the operation and the fault's code,
refined by the fault messages `internal/node/git` exports for causes that share
a code (branch exists, branch checked out in another worktree, destination
exists, locked, uncommitted changes, path still exists). `workspaceFailure`
owns that mapping and `mutateNamed` applies it; file operations, the Git
pending re-check and conversation moves keep their generic toasts.

Default tests use temporary go-git repositories, injected native responses and
static argv. Integration-tagged probes execute native Git only in disposable
repositories; `TestLargeRepositoryIntegration` covers status, change inventory
and staging with 20,000 tracked files and branch listing over 150,000 commits. Windows Job behavior requires the Windows probe; local compilation
alone does not establish another operating system's behavior. Build and test
results belong to the coordinator's stage report.
