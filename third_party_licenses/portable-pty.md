# Portable PTY patches

Sailry uses `portable-pty 0.9.0` from the pinned
[sailry/wezterm fork](https://github.com/sailry/wezterm/tree/e814e06b5149d73ddbcb5be670b41855b2873c74/pty),
based on [WezTerm](https://github.com/wezterm/wezterm/tree/e2a98e1b8af156988ad0ac778658ac27b60a55fb/pty).
The fork adapts upstream proposals [#7604](https://github.com/wezterm/wezterm/pull/7604)
for process creation flags and [#7606](https://github.com/wezterm/wezterm/pull/7606)
for the retained primary thread handle, alongside Sailry's Windows Job and ConPTY
fixes below. The dependency version remains pinned at 0.9.0. The complete upstream
MIT notice is retained in [portable-pty-LICENSE.md](portable-pty-LICENSE.md).
The fork's `portable-pty` depends on its `filedescriptor 0.8.3` workspace package;
both packages retain the same MIT notice.

The Windows patch lets a command retain a caller-owned Job. ConPTY creates
that process with `CREATE_SUSPENDED`, assigns it to the Job, then resumes its
retained primary thread. Assignment or resume failures terminate the suspended
process and propagate the error. This closes the interval in which an interactive
command could create descendants before Node attached its cleanup Job.
The Job handle stays owned until command cleanup; builders without a Job preserve
the upstream startup behavior. Unix PTY creation is unchanged.

`ConPtySystem` additionally allows cursor inheritance to be disabled when a host
does not interpret or answer the pseudoconsole's cursor query. The default retains
the upstream flags; Sailry uses the opt-out only for managed commands. Command
cleanup closes the pseudoconsole on a blocking worker with its reader retained,
then waits for final output before stopping I/O. This follows the Windows
[creation](https://learn.microsoft.com/en-us/windows/console/createpseudoconsole)
and [closure](https://learn.microsoft.com/en-us/windows/console/closepseudoconsole)
contracts and keeps existing terminal startup behavior unchanged.
The master drops its original output reader before the pseudoconsole. A running
cloned reader still drains final output; startup failure without a reader instead
breaks the output channel before closure, avoiding a wait on an absent reader.
