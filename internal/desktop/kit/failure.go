package kit

import (
	"errors"
	"log/slog"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/node/logging"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Fail shows the outcome named by key and records the failure behind it, so
// a toast the user saw can be traced from the log and error reports.
func Fail(c *ui.Context, outcome Outcome, key string, err error) {
	Toast(c, outcome, i18n.T(c, key))
	RecordFailure("desktop", key, outcome, err)
}

// RecordFailure logs a failure shown to the user under its stable operation
// name. A failed outcome is an error, which error reporting sends with only
// the area, operation and code; the fault message stays in the local log.
// Conflicts, uncertain outcomes and cancellations are warnings.
func RecordFailure(area, operation string, outcome Outcome, err error, attrs ...slog.Attr) {
	code := string(FaultCode(err))
	if code == "" {
		code = "internal"
		if err == nil {
			code = "unspecified"
		}
	}
	level := slog.LevelError
	if outcome != Failed || code == string(protocol.ErrorCodeCancelled) {
		level = slog.LevelWarn
	}
	attrs = append(attrs, slog.String("area", area), slog.String("operation", operation),
		slog.String("phase", logging.Failed), slog.String("code", code))
	if err != nil {
		attrs = append(attrs, slog.String("detail", err.Error()))
	}
	logging.Emit(level, attrs...)
}

// FaultCode is the protocol code an error carries, or "" for another error.
func FaultCode(err error) protocol.ErrorCode {
	var pointer *protocol.Fault
	if errors.As(err, &pointer) {
		return pointer.Code
	}
	var fault protocol.Fault
	if errors.As(err, &fault) {
		return fault.Code
	}
	return ""
}
