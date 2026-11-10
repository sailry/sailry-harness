# Projects and worktrees

The execution Node registers canonical project directories and owns their
worktree associations. Native clone runs outside the database writer so Git
credential helpers and SSH configuration remain available. The existing durable
admission outcome records registration only after the external clone is verified.
An incomplete clone remains available for inspection and is never replayed under
a new request identity.

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
