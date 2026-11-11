# Delegated execution

The execution Node admits children against the parent turn's immutable role,
provider, model, worktree, permission and package capture. Children use the same
Agent Runtime claim pass and turn loop as main sessions. Store owns canonical
ancestry, revision conflicts, child admission, current per-root concurrency and
FIFO slots. Controllers never start a second execution loop.

The core `spawn_agent`, `guide_agent` and `wait_agent` tools are available to
unbound main sessions with captured Team execution. Plan retains ordinary write
restrictions while permitting this captured delegation control. Children cannot
delegate. Read-only roles enter Plan; explicit role tool selections are checked
against the captured catalog. Role instructions and selected complete skill
documents join the stable policy. Role action budgets retain the source prompt
and termination behavior.

A package tool bound to `agents.delegate` (the code-review package's
`delegate_review`) admits a child through the same path. Its host script
prepares the child's arguments from the ones the model sent. The recorded call
keeps the name and arguments the model sent and locates the child's origin; the
prepared arguments describe the child. Store accepts a package alias only when
the parent turn's captured packages bind it to `agents.delegate`.

A package tool bound to `agents.guide` reaches `guide_agent` the same way: its
script prepares `session` and `message`, a running child is guided, a settled
one is resumed, and the result names the `recipient` turn. Store accepts the
alias only when the captured packages bind it to `agents.guide`, and a session
is required. `agents.wait` is not bound for packages.

Foreground spawns return admitted identities and canonical pending receipts.
The complete parent tool batch settles before its children become executable.
Children from that response form one concurrent wave. Their durable final
responses arrive as separate canonical messages with delivered or cancelled
operation notes before the parent requests another model response. Actual main
guidance can interrupt collection at the existing guidance boundary: the step
restarts, the guidance joins the next model request, and collection resumes
after that response. Stopping the parent during collection settles its
unanswered calls before the cancellation notes.

Running guidance receipts identify the new queued turn and the active recipient
separately. Guidance admitted after a child's last input boundary passes to a
successor under the same cancellation owner before completion is published.
The ordinary child claim pass still enforces its FIFO concurrency slot, and
same-boundary inputs join that successor before inference. Foreground collection
follows the exact execution chain. Failed or stopped children retain unconsumed
guidance in the paused queue, with no automatic restart; collection still returns
the actual terminal child outcome. Guiding an idle child creates foreground work
that remains unreleased until the parent's canonical tool batch is committed.

Admission and its handoff remain Node-owned when the calling tool is cancelled.
The parent's operation lease covers that handoff and ends before a detached
background child's lifetime. Background children survive their parent's normal
return; ordinary foreground children share parent cancellation. Child panic
settlement drains admitted effects and records an uncertain outcome without
replay.

A parent turn that is stopped or fails cancels every child of its session:
foreground and background, running and queued, including background children
earlier turns left running. Children cannot delegate, so this is the whole
subtree. The parent waits until each of them has settled before its own run is
finished, so no later turn starts beside a child that is still closing its
tools. Their completion records are consumed with the parent: the cancelled
work does not join the next message and never starts a notice turn. A child
that finished before the stop keeps its pending result. Node shutdown is not
waited for; it interrupts children through recovery.

`stop_turn` on a child's turn stops that child only; its siblings and its
parent keep running. A running child stops like any turn. A queued child that
never started is marked cancelled without a finish time, and the next claim
pass finishes it from the store, which closes its completion record and
refreshes the parent; until then the child counts as busy and its result is
not collected. Queued guidance for a child ends where it is stopped, because
it shares its recipient's completion record. A foreground parent receives the
ordinary cancellation note; a background child's cancelled result joins the
parent's next message and never starts a notice turn by itself.

`wait_agent` observes existing owned work for 150 seconds through the injected
clock. This window does not stop child execution. It returns the same session
and turn while work remains pending, returns early for actual durable parent
guidance, and accepts an exact notice turn for result collection. Background
evidence is consumed only after its result is delivered canonically.

Store reserves background notices in sequence order, at most 16 notices and
32 KiB of rendered untrusted JSON. Engine appends the exact reserved text with
its notice part and identities. Store confirms delivery in the canonical append
transaction. Optional automatic continuation reuses ordinary admission and
preserves real completion when scheduling fails. Package callback execution is
owned by its later runtime phase.

Captured localized tool metadata uses the existing source locale keys. Native
child approval and question updates also refresh the owning parent's shared
conversation feed; ordinary approval ownership remains on the child session.

When `apply_agents` or `apply_team` is offered, the delegation context ends
with the captured conversation revision and the rule that catalog changes do
not alter the running turn's roster. `list_roles` is among the operations a
restricted child role may be granted. The role and team catalog tools are
described in [Teams and roles](teams.md#agent-catalog-tools).

## Differences from the Rust runtime

- A failed parent turn cancels the session's children as a stopped one does,
  as in Rust. A turn whose only failure is its completion hook does not: the
  hook runs after children are settled and Go does not settle them twice.
