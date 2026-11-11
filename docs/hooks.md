# Workspace hooks

`internal/node/hooks` runs the scripts captured with an admitted task turn. The
Node supplies immutable settings, scope and durable Claim/Finish callbacks.
Scripts execute through the managed Process service outside SQL transactions.
Hooks always request workspace access, including captured hook environment and
timeout, and retain the managed-worktree protection used by ordinary commands.
Darwin and Linux report workspace sandbox support; Windows reports its current
command policy. A missing sandbox launcher fails rather than widening access.
Plan mode and non-task turns cannot claim execution. Updating settings affects
new admission only.

The existing Before Tool, After Tool and Turn End slots precede additional
scripts in saved order. Before Tool failures prevent that tool call. After Tool
failures are retained without replacing its result, and contribute to the final
turn status. Turn End uses ordinal zero. Script metadata contains only the
source `SAILRY_HOOK_*` identity, phase, tool and status fields.

`SAILRY_HOOK_EVENT` is `before_tool`, `after_tool` or `turn_end`.
`SAILRY_HOOK_STATUS` is absent before a tool, `succeeded` or `failed` after
one, and `completed` or `failed` at Turn End, which the engine passes for a
completed turn and for any other end. An After Tool failure makes the Turn End
status `failed`.

No script runs once the turn is stopped: the binding holds the turn's context,
which a user Stop and Node shutdown both cancel, and a claim is refused unless
the run is still `running`. A child under a read-only role is created in Plan
mode and its conversation cannot be reconfigured, so the Plan rule also covers
it; no separate role test is needed.

The existing writer stores a claim before effects and a terminal receipt after
effects. An existing uncertain claim cannot be launched again. Completion
recording survives cancellation of the effect. A panic or unknown outcome is
reported as uncertain without replay. Settings saves use revision conflicts and
preserve failed editor drafts through the existing Actions contract.

`store/workspace_hooks_settings.go` owns revisioned settings, frozen capture and
hook receipts. `workspace_process.go` binds these callbacks to the existing
writer and Process service. The Agent engine invokes the captured before/after
hooks around its tool batch and the end hook before durable turn completion.
The live workspace settings adapter maps stored scripts and revisioned saves
to the accepted editor and consent dialog. Controller layout, copy and Actions
remain the accepted desktop contract.
Project-backed unsent drafts use the same worktree resolution as the workspace
panel. Consent identifies the actual Node, root and worktree, retains the known
branch label, and selects its scope text from the Node's reported capability.
The Node identity is used as its label until host labels are available.

Fixture tests cover admission capture, uncertain claims, the status words and
the stopped turn. No extra approval
system, retry policy or hook execution engine is introduced.
