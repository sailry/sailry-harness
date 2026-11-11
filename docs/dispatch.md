# Durable dispatch

`internal/node/dispatch` ports the reviewed Rust calendar and original fixtures.
The November 10, 2026 user decision authorizes this source port in place of
gocron. Desktop and Host share this calendar authority.

Weekly schedules use IANA zones and local weekdays/wall time. Missing times are
skipped; ambiguous times select the earlier occurrence and consume it once.
A wall time inside a daylight-saving gap, or on a day the zone never had, has
no occurrence that day and is not moved to another time: a daily 02:30 does not
run on the night the clocks go forward. Rust does the same (`calendar.rs`,
`earliest()`).
A late observation coalesces missed ticks to the latest due occurrence, retaining
exact `scheduled_ms`. Durable cursor consumption leaves the user revision intact.

The existing Node writer owns handlers, schedules, events, jobs, claimed requests
and recovery. Claims freeze package/settings authority and the exact callback.
Recovery does not resubmit uncertain external effects. Full queues leave due
cursors unconsumed. Stopping a dispatched turn resolves its recorded job owner.

Committed changes, callback completion and the next due time wake the runner.
Production uses standard timers through clockwork's real clock; tests use its
fake clock. Callbacks return through ordinary Node admission. Turn-completion
callbacks observe canonical status without another execution loop.

## Wall clock, sleep and catch-up

Due times are wall-clock instants, and a timer counts monotonic time, which
stops while the system sleeps and ignores a clock change. The runner therefore
never waits longer than `dispatch.Recheck` (30 seconds) on one timer: each wait
ends in a tick that reads the wall clock again. A fire that became due while
the system slept, or after the clock was set forward, runs within 30 seconds of
the wake; a clock set back does not run anything early. Desktop also wakes the
runner when the system reports a resume (`Node.CheckSchedules`), so there the
fire is immediate; the Host has no such signal and relies on the recheck. A
tick with nothing due changes nothing.

Catch-up is one run per schedule, never one per missed occurrence. A schedule
whose time passed while the system slept, or while no Desktop or Host was
running, fires once at the next tick with the `scheduled_ms` of its latest
missed occurrence and moves its cursor to the next future one. A one-time
schedule that was missed fires once, late. Rust has the same policy
(`store/dispatch/schedules.rs`, `occurrence`).

Deviation from Rust: Rust arms one `tokio::time::sleep` for the whole delay
and has no recheck. The delay there is hidden by a wake of the runner on every
admitted request and by a keep-awake that holds whenever its setting is on;
neither holds for an idle Go Node, so the recheck replaces both.

## Restart, edits and removal

A tick consumes a due occurrence and claims its job in one write, so a restart
cannot fall between the two. A callback that was claimed but whose outcome was
not recorded when the Node stopped is not replayed: its job becomes `unknown`,
with one error notice, and the schedule goes on with its next occurrence. A
turn the callback had already admitted is resumed and observed to its end. An
occurrence therefore runs at most once; "exactly once" is not promised for a
callback interrupted by a stop, as in Rust.

A job carries the event, payload and callback it was fired with. Editing a
schedule's payload or switch keeps its cursor and does not touch a run in
flight; a new timing starts the cursor over (`dispatch.First`), and if that is
due at once while the previous run is unfinished the occurrence is skipped
under the overlap rule rather than started beside it. Removing a schedule
stops later occurrences only: its run in flight finishes and is recorded.

A one-time schedule fires once, late if its time was missed, and then has no
next time; saving it again with the same time does not fire it again, a new
time does. The reminders package builds on exactly this: one `once` schedule
and one handler per reminder, completion disables the schedule, a changed due
time is a new timing, and its callback ignores a fire whose reminder changed
meanwhile. It has no snooze, here or in Rust.

## Overlap

A scheduled occurrence never overlaps the previous run of its own handler.
While that handler still has a queued or running job, the occurrence is
consumed and recorded as a job that never starts: status `cancelled`, fault
code `busy`, `finished_ms` set and `started_ms` unset. An "every 5 minutes"
task whose run takes 20 therefore runs once and records three skipped
occurrences. Run now (`enqueue`) and published events are not schedule
occurrences and always start. A skipped occurrence raises no notice; it shows
in the package's job history.

A handler's `queue` is a label for its package's own grouping. It is stored
with each job and orders nothing: handlers never wait for each other. The
official packages put every task in one queue (`default`, `reminders`), so a
serial queue would let one run that waits for approval hold back every other
task. The overlap rule is per handler instead. Removing the field needs a
contract and SDK change and is not done here.

Deviation from Rust: Rust starts every occurrence and has no overlap rule;
its `queue` is the same unused label.

## Notices

A scheduled run has nobody watching it, so the Node reports it durably
([Node notices](node.md)): the run's own completed, failed or interrupted
notice; one notice while a dispatch-started turn waits for an approval or an
answer; and one error notice for a job that failed, or whose outcome is
unknown, before any turn existed. An unanswered approval is cancelled and a
question times out after 300 seconds, after which the run continues without
it and the notice stays unread; only plan acceptance waits without a limit.
While a run waits its job stays `running` and its handler's next occurrences
are skipped.
