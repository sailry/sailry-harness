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
