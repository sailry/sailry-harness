# Durable dispatch

`internal/node/dispatch` ports the reviewed Rust calendar and original fixtures.
The November 10, 2026 user decision authorizes this source port in place of
gocron. Desktop and Host share this calendar authority.

Weekly schedules use IANA zones and local weekdays/wall time. Missing times are
skipped; ambiguous times select the earlier occurrence and consume it once.
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
