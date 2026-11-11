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

Core Files and Repository subscriptions share one root watcher. The desktop's
branch/worktree projections and native badge consume these Node-owned services;
plugins do not own native panel state. Native lifecycle behavior is covered by
integration-tagged temporary-repository probes and must run on each supported OS.
