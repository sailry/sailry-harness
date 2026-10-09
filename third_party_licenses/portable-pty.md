# Portable PTY patches

`vendor/portable-pty` contains the published `portable-pty 0.9.0` package from
[WezTerm](https://github.com/wezterm/wezterm/tree/f8921727a11b9f8b073e8c24821d72fd41283500/pty).
The complete upstream MIT notice is retained in `vendor/portable-pty/LICENSE.md`.
Registry cache files are omitted. The dependency version remains pinned at 0.9.0.

The local Windows patch lets a command retain a caller-owned Job. ConPTY creates
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
