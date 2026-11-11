package node

import (
	"log/slog"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// A bundled server nobody connected is tried at every start; that it waits
// for its account is no warning, while a configured server that fails is.
func TestWarmFailureLevel(t *testing.T) {
	t.Parallel()
	for code, level := range map[protocol.ErrorCode]slog.Level{
		protocol.ErrorCodeNotConfigured:    slog.LevelDebug,
		protocol.ErrorCodeUnavailable:      slog.LevelWarn,
		protocol.ErrorCodePermissionDenied: slog.LevelWarn,
		protocol.ErrorCodeInternal:         slog.LevelWarn,
	} {
		if got := warmFailureLevel(code); got != level {
			t.Errorf("%s logs at %s, want %s", code, got, level)
		}
	}
}
