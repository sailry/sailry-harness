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
// name. A failed outcome is an error. A Node-issued reference may accompany
// the public diagnostic fields; the fault message stays in the local log.
// Conflicts, uncertain outcomes and cancellations are warnings.
func RecordFailure(area, operation string, outcome Outcome, err error, attrs ...slog.Attr) {
	fault := failureFault(err)
	code := string(fault.Code)
	if fault.Code != protocol.ErrorCodeCancelled {
		if logging.ValidReportRef(fault.ReportRef) {
			attrs = append(attrs, slog.String("report_ref", fault.ReportRef))
		}
		if area == "desktop" && operation == "inspect_git" {
			reason := fault.GitInspectionReason
			if stage := reason.Stage(); stage != "" {
				attrs = append(attrs, slog.String("stage", stage), slog.String("reason", string(reason)))
			}
		}
	}
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
	return failureFault(err).Code
}

func failureFault(err error) protocol.Fault {
	var pointer *protocol.Fault
	if errors.As(err, &pointer) && pointer != nil {
		return *pointer
	}
	var fault protocol.Fault
	errors.As(err, &fault)
	return fault
}
