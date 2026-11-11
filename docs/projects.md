# Projects and worktrees

The execution Node registers canonical project directories and owns their
worktree associations. Native clone runs outside the database writer so Git
credential helpers and SSH configuration remain available. The existing durable
admission outcome records registration only after the external clone is verified.
A clone is never replayed under a new request identity.

The Node creates the clone destination itself and removes it when the clone
does not finish, so a retry finds the path free. The reference retains the
partial directory and reports every clone failure as an uncertain outcome; here
only a stopped process is uncertain (the stall limit below, or Node shutdown).
When Git itself answers, the failure is plain: refused access is
`permission_denied`, and a wrong URL, an unreachable host or a missing branch
is `invalid_request`. Git's words stay in the ephemeral output, because a
durable receipt carries no remote URL.

A clone has no total deadline; the reference's libgit2 clone has none either.
It runs with `--progress` and is stopped only after two minutes without any
output (see [Git](git.md)). The protocol has no command that cancels a project
effect, so a user cannot stop a running clone; it ends with the Node.
A Node that is killed during a clone cannot remove the destination: at the next
start the request is `unknown`, nothing records that the directory was the
clone's, and a retry is refused because the path exists until the user deletes
it. The Node does not delete a directory at a user-chosen path on the strength
of an earlier process.

Linked worktree lifecycle uses native Git. A captured project and reserved
worktree ID identify a managed checkout beneath the main worktree's
`.sailry/worktrees/<id>` directory. The service appends `.sailry/` to Git's
`info/exclude`, preserving existing entries and user ignore files. It rejects
symlinked metadata directories and destinations that already exist.

Managed creation can copy staged and working changes from an observed source
HEAD/index into a new checkout. It never stashes or edits the source. A selected
base commit cannot also carry source changes. Failures after creation report an
uncertain outcome and retain the checkout for inspection.

Removal verifies the captured HEAD/branch, clean status, native registration,
locks, nested checkouts and active Node resources. The main worktree cannot be
removed. SQL finalization keeps tombstones when history still references a
removed checkout; re-registering the same project path restores those retained
associations. The native action and database publication remain separate, and a
failed finalization never authorizes replaying external removal.

Removing a project keeps its files, sessions and worktree rows, and closes what
ran in it, as its confirmation says. The Node validates the removal first (a
changed project or one with a queued or running turn is refused and nothing is
closed), then closes the project's running terminals and stops the background
commands of its sessions, then records the removal. Terminal records left by an
earlier process are closed in the recording transaction. A removal that fails
after a terminal was closed carries `terminals_may_be_closed`. Two differences
from the reference: it does not stop background commands, and it sets
`terminals_may_be_closed` on every late failure, whether or not the project had
a terminal.

Durable workspace effects are ordered per worktree root. Effects on one root
run one at a time, as before; effects on different roots run together, and an
effect across two roots takes both in a fixed order. Commands without a root
share one order among themselves. A registration (a project created,
registered, updated or removed, a worktree created, registered or removed)
excludes every effect, because path protection reads which roots exist; a
project command holds that exclusion only while its result is recorded, not
during its clone or its checks. An effect that waited through a registration
resolves its roots again and reports a conflict when they changed. The
reference orders all of this with one Node-wide lock, which the clone and every
transfer with a remote held too, so one push delayed every agent file write.

Transfers with a remote hold no root: fetch, push and the pushes that delete a
remote tag or branch need none, and a pull (also the pull of a sync and of a
commit's sync) first fetches without it and then runs `git pull` under it. That
second step finds the objects present, so the root is held for a short
negotiation and the merge or rebase. The cost is a second contact with the
remote, which a credential that asks for each connection answers twice.

Deleting a session removes what only it used, once its background commands
have stopped; archiving removes nothing. The private workspace of a session
without a project, `workspaces/sessions/<session>` in the profile, is deleted,
and a directory an earlier process could not delete goes at the next start. A
managed worktree, `.sailry/worktrees/<id>` in the project, is removed exactly
as Remove worktree does it and under the same conditions: no remaining
session uses it or used it in an earlier revision, and Git reports it clean,
on a branch, unlocked and with no operation under way. Its branch and commits
stay. A worktree with uncommitted or untracked files is kept, and the worktree
manager marks every linked worktree no session uses as `No session`, where it
can be opened or removed. The workspace of a plugin assistant or a saved
connection is shared by its conversations and is never removed with one. The
reference keeps all of these directories forever and shows nowhere which
worktrees are unused. The delete confirmation does not yet say that the files
of a session without a project go with it.

Core Files and Repository subscriptions share one root watcher. The desktop's
branch/worktree projections and native badge consume these Node-owned services;
plugins do not own native panel state. Native lifecycle behavior is covered by
integration-tagged temporary-repository probes and must run on each supported OS.
