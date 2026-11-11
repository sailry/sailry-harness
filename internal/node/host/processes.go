package host

import (
	"context"
	"math"
	"os"
	"runtime"

	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/shirou/gopsutil/v4/process"
)

// StopProcess signals one process that the detailed metrics listed. The start
// time is part of its identity, so a reused process ID is refused. Operating
// system permissions decide whether the signal is delivered.
func StopProcess(ctx context.Context, pid uint32, startedAtSecs uint64, force bool) error {
	if pid <= 1 || pid == uint32(os.Getpid()) {
		return &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "cannot stop the system or the execution Node"}
	}
	// Windows has no termination request; only the forced stop exists there.
	if !force && runtime.GOOS == "windows" {
		return &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "process signal is unsupported on this host"}
	}
	missing := &protocol.Fault{Code: protocol.ErrorCodeNotFound, Message: "process is no longer running"}
	if pid > math.MaxInt32 {
		return missing
	}
	target, err := process.NewProcessWithContext(ctx, int32(pid))
	if err != nil {
		return missing
	}
	started, err := target.CreateTimeWithContext(ctx)
	if err != nil {
		return missing
	}
	if startedAtSecs == 0 || started < 0 || uint64(started/1000) != startedAtSecs {
		return &protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "process identity changed; refresh before retrying"}
	}
	if force {
		err = target.KillWithContext(ctx)
	} else {
		err = target.TerminateWithContext(ctx)
	}
	if err != nil {
		return &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "could not signal process; it may have exited or require higher privileges"}
	}
	return nil
}
