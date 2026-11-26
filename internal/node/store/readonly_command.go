package store

import (
	"context"
	"database/sql"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// TurnIsPlanMode reports whether turn was captured in planning mode (which also
// includes read-only delegated child turns).
func TurnIsPlanMode(ctx context.Context, db *sql.Conn, turn protocol.TurnID) (bool, error) {
	capture, err := readTurnCapture(ctx, db, turn)
	if err != nil {
		return false, err
	}
	return capture.Config.Mode == protocol.WorkModePlan, nil
}

// CheckPlanRunCommand permits only foreground commands bound to the read-only
// workspace sandbox. Shell text cannot establish read-only authority. The
// launcher must enforce the OS sandbox or fail without starting the shell.
func CheckPlanRunCommand(input protocol.CommandRunCommandPayload, sandboxSupported, sandboxEnabled bool) error {
	switch {
	case input.Background || input.Service || input.Pty:
		return fault(protocol.ErrorCodePermissionDenied, "planning mode permits only foreground read-only commands")
	case input.Access != protocol.ProcessAccessWorkspace:
		return fault(protocol.ErrorCodePermissionDenied, "planning mode permits only read-only sandboxed workspace commands; unrestricted access is not allowed")
	case !sandboxSupported || !sandboxEnabled:
		return fault(protocol.ErrorCodePermissionDenied, "planning mode permits only commands in an enabled read-only OS sandbox")
	default:
		return nil
	}
}
