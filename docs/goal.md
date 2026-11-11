# Goal

`/goal <text>` sets one goal for a conversation, and the session keeps working
on it, turn after turn, until the goal is complete or the user closes it. A
message, a Stop or a pause in between stays inside the goal.

The feature is the bundled `goals` package (`plugins/goals`). Core has no
goal-specific code: it provides generic services any package loop can use
([Core services](#core-services)).

## Interaction

| Moment | What happens |
|---|---|
| `/goal <text>` | The goal is stored and the text is sent as the first turn. In Plan mode that turn plans; pursuit starts when a turn runs outside Plan, which the accepted plan's turn is |
| `/goal <new text>` over an unfinished goal | Replaces it; the old one goes to the session's history as Replaced |
| `/goal` | Opens the panel |
| A goal turn ends | The package admits one continuation turn, shown in the transcript as the marker row "Continue goal" |
| A message during the goal | Guidance joined into the running turn, or the user's own turn after it. It never changes the objective. The goal continues after it |
| Stop | The turn stops and the goal pauses, reason Stopped. A reply or Resume continues it |
| Pause (panel, `/goal pause`, or asking the model) | Holds the goal until Resume. Replies are ordinary turns |
| Claiming completion | Two steps: the working model calls `update_goal(complete, evidence)`, which records a claim; the next turn is the audit turn "Verify goal", which must call it again. An audit that does not confirm drops the claim and work goes on |
| Reopen (panel, `/goal resume` on a completed goal) | The same goal becomes active again with its totals |
| Edit (panel, `/goal edit <text>`) | The objective changes from the next turn, which is told once |
| Clear (panel, `/goal clear`) | A running goal turn is stopped; the goal goes to history as Cleared |
| Approval or question | The turn waits inside itself. One that expires unanswered pauses the goal, reason Needs you |
| Background work still running | The goal waits instead of continuing. The notice turn that delivers the result wakes it |
| Restart | The goal is shown paused, reason App restarted. Nothing resumes by itself |
| Fork or rewind | The restored goal is paused, reason Restored from history, until Resume |
| Plan mode | A goal does not run in Plan. A turn that ends in Plan pauses it, reason Plan mode; the next turn outside Plan resumes it |

## States and reasons

Stored `status` is `active`, `paused`, `blocked` or `completed`; `paused`
carries a `reason`. The shown states of `active` are derived: Running, Waiting
(for background work or a turn still to end) and Verifying (a claim awaits its
audit).

| Reason | Set when | Ends with | Notifies |
|---|---|---|---|
| `stopped` Stopped | The user stopped a goal turn, or the queue is paused | A reply, Resume | No |
| `user` Paused by you | Pause, `/goal pause`, `update_goal(paused)` in a user turn | Resume, `/goal resume`, `update_goal(active)` in a user turn | No |
| `turns` Turn limit reached | The wrap-up turn of the turn limit ended | A reply, Resume | Yes |
| `tokens` Token budget reached | The wrap-up turn of the token budget ended | A reply, Resume: the budget is granted again | Yes |
| `time` Time limit reached | The wrap-up turn of the time limit ended | A reply, Resume: the limit is granted again | Yes |
| `no_progress` No progress | Two automatic turns in a row made no tool call other than the package's own, with no background work pending | A reply, Resume | Yes |
| `needs_you` Needs you | An approval or question expired unanswered in a goal turn | A reply, Resume | Yes |
| `error` Error | A goal turn ended `failed` (the fault code is kept), or the session cannot take a continuation (archived, worktree gone) | A reply, Resume | Yes |
| `restart` App restarted | A goal turn ended `interrupted`, or the goal is found active after its last turn was lost | A reply, Resume | No |
| `plan` Plan mode | A turn ended in Plan mode, `/goal` or Resume in Plan mode | The next user turn outside Plan | No |
| `restored` Restored from history | Fork or rewind | Resume | No |

`blocked` is set by the model with what it needs; a reply or Resume continues,
and it notifies. `completed` notifies once, with the evidence the audit stated.
One notification is published per event, for the session, with a localized
heading ("Goal paused · Turn limit reached"), the objective's first line as
title and the detail as message.

A goal whose loop cannot be running is never shown as running. The package's
reads derive `paused` when the stored status is `active` but the session's
latest turn ended `interrupted`, `failed` or `cancelled`, or the value was
restored by a fork or rewind; Resume or a reply then continues it.

## Who may do what

| Action | User | Model |
|---|---|---|
| Start | `/goal <text>` | `create_goal(objective)`, only in a turn the user started, never over an unfinished goal, and the call asks for the ordinary tool approval |
| Replace, Edit, Clear, Reopen | Yes | No: there is no tool for it, and `update_goal` takes no objective |
| Pause, Resume | Yes | `update_goal(paused)` and `update_goal(active)` only in a turn the user started; refused `permission_denied` in an automatic turn |
| Complete | Reopen undoes it | `update_goal(complete, note)`: a claim, then the audit |
| Blocked | A reply or Resume continues | `update_goal(blocked, note)` |
| Read | Panel, badge | `get_goal` |

In Plan mode the model is offered `get_goal` only.

## Limits

Package settings (`settings.json`): `turn_limit` (default 25, at most 100),
`token_budget` in thousands of tokens and `time_limit` in minutes of active
time (both 0, off, by default).

- The turn limit counts automatic turns, notice turns and continuations alike,
  since the user's last message; any user input starts a new count. Every
  continuation declares this limit to the Node, which enforces it.
- The last allowed turn is told to wrap up: bring the work to a safe state and
  report what is done, what remains and the next step. When it ends the goal
  pauses with the reason. A limit never stops silently and never completes the
  goal.
- Token and time budgets count from the goal's start. Resuming a goal paused
  at one grants it again.
- Time counts while the status is `active`; tokens are the input and output of
  goal turns, delegated children included.

## Prompts

All model-facing text is English, in `host/prompts.js`.

- The objective is a pinned context part from `turn.initialize`, with its
  version; the Node adds it once and again only after compaction hides it. An
  edit adds the new version.
- The per-state instruction is fixed text, so it enters the context once per
  state: active (keep working, a final answer does not end the goal, messages
  are guidance, do not redefine success, how to claim completion and when to
  report blocked), on hold with the reply resuming, on hold until the user
  asks, planning, and completed.
- A continuation message states the counters ("automatic turn 7 of at most
  25", token and time use when budgets are set), what changed (resumed,
  objective edited, the audit did not confirm) and, on the last turn, the
  wrap-up instruction.
- The audit message quotes the claim, tells the model to treat it as unproven
  and to check every requirement against current evidence, and to call
  `update_goal(complete)` only if all are met.

## Storage

Conversation values of the package, which follow fork and rewind:

- `goal`: objective and its version, status, reason, detail, the sequence of
  the last turn handled, the pending claim, evidence, counters (goal turns,
  automatic turns, tokens, active time and their budget windows) and flags
  for the next continuation (edited, wrap-up, waiting).
- `history`: the last 20 goals the session completed, replaced or cleared,
  with objective, outcome, turns, time, tokens and evidence.

A queued continuation is fenced on `goal`'s revision: any later write to it,
for example a Pause, withdraws a continuation that has not started.

## Core services

| Service | Owner | Contract |
|---|---|---|
| Turn facts | `node.afterTurn`, `store.ReadTurnFacts`, `engine.TurnObservation` | `after_turn` receives the turn's `sequence`, `status` (`cancelled` is the user's Stop, `interrupted` a lost turn, `failed` with `error`), `mode`, `automatic`, `notice`, the package's own `continuation` marker, `tool_calls` and per-name `tools`, `expired` approvals and questions, `usage` and `session_usage`, `background.running` and `background.pending`, and the automatic `chain` |
| Turn origin at start | `node.pluginTurnInput` | `turn.initialize` receives `automatic`, `notice` and the package's own `continuation` |
| Automatic chain | `store.ReadAutomaticChain`, `store.admitContinuation`, `store.admitBackground` | Notice turns and continuations share one count since the last user input, eight by default. A continuation declares `limit` (1 to 100) for the chain it joins; notice turns follow the latest declared limit. A continuation with `user` set, one the user asked for, starts a new chain |
| Refusal reasons | `store.automaticRefusal`, `store.continuationRefused` | A refused `continue_turn` carries `details: {kind: "continuation", reason}`: `plan`, `child`, `archived`, `worktree`, `queue_paused`, `queued_input`, `running`, `interaction`, `limit` (with `count`, `limit`) or `stale` |
| Results reach a continuation | `store.BindBackground` | A continuation takes the session's waiting background results, held ones included, as a user turn does |
| Named markers | manifest `continuation_markers`, `continue` `marker` | A continuation selects one of the package's declared markers; the run records its id and label |
| Marker row | `live.Conversation`, `conversation.Turn.Marker` | The transcript shows the marker in the window's language in place of the continuation's input; a queued continuation is an automatic queue row with the same label |
| Tool approval | manifest handler `approval` | A plain handler can ask for the ordinary tool approval |
| Attention | `store.claimAgentInvocation` | A started automatic turn takes back the unread mark its predecessor raised; the turn that ends the chain raises it |
| Notifications | `notify` transaction operation | Unchanged; the package declares `notifications.publish` |

Callbacks of consecutive turns can overlap, because a queued turn starts as
soon as its predecessor ends. The package commits each decision against the
goal's revision, decides again after a lost race, and ignores the callback of
a turn older than one it already handled, by `sequence`.

## Tests

| Covers | Test |
|---|---|
| The loop on a real Node with the bundled package and a scripted model: claim and audit, an inserted message, Stop and reply, Pause across a reply, the 25-turn limit, waiting for background work, restart, what the model cannot do, Plan mode, `create_goal` approval | `node.TestGoalLoop` |
| Turn facts and origin | `node.TestPluginTurnFacts` |
| Chain count, declared limit, user-requested continuation, refusal reasons, markers, held results, attention | `store.TestAutomaticChain` |
| A turn submitted inside a plugin transaction | `store.TestPluginTransactionSubmit` |
| Manifest markers | `plugins.TestContinuationMarkers` |
| The state machine, prompts, badge, history | `plugins/goals/tests/host.test.mjs` |
| Panel and controller | `plugins/goals/tests/view.test.mjs`, `controller.test.mjs` |
| Marker row and projection | `conversation.TestContinuationMarker`, `live.TestContinuationMarker` |
| Demo goals | `preview.TestDemoGoals` |

## Deviations

From the Rust reference:

| Difference | Reason |
|---|---|
| Six model tools became three; the model cannot edit, clear, replace or reopen | The objective is the user's; a model that can rewrite it can redefine success |
| Completion needs a separate audit turn | A final answer or a single call is too easy to make early |
| After Stop a reply continues the goal | Stop already means "nothing runs until the user sends or resumes"; the goal follows the same rule |
| Limits, wrap-up, no-progress and pause reasons are new | The reference enforced no budget and stopped without saying why |
| The loop advances in `after_turn` from stated facts, not by paging run history | One read, no history scan, and the facts include what history does not hold |
| `/goal` is accepted in Plan mode | The first turn plans and pursuit starts with the accepted plan |

From Codex:

| Difference | Reason |
|---|---|
| A reply after Stop resumes; Codex needs `/goal resume` | One action to redirect a goal instead of two |
| The default limit is 25 automatic turns, not a token budget | A turn count needs no pricing data and bounds every provider; token and time budgets are optional |
| Completion is audited in a separate turn by the same model | Stronger than a single `update_goal`; an independent verifier is not built |
| The feature is a package over generic Node services | Plugins own their workflows and prompts; core provides scoped services |

## Not covered

- A goal waiting on background work that ends without a deliverable result
  (for example a command the user cancelled) is not woken; it shows Waiting
  until a reply or Pause and Resume.
- A turn that completes at the instant the Node stops can lose its callback;
  the goal then shows Running with no turn until a reply or Pause and Resume.
- The no-progress rule counts tool calls; a model that calls a trivial tool
  every turn runs until the turn limit.
- Real providers, Windows and Linux are not verified by the tests above.
