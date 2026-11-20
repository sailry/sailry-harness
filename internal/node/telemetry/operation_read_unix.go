//go:build unix

package telemetry

import "golang.org/x/sys/unix"

// A raced symlink or FIFO must not redirect or block the queue worker.
const operationReadFlags = unix.O_RDONLY | unix.O_NOFOLLOW | unix.O_NONBLOCK
