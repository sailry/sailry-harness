package kit

import (
	"context"
	"errors"
	"log/slog"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/node/logging"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Fail shows the outcome named by key and records the failure behind it, so
// a toast the user saw can be traced from the log and error reports.
func Fail(c *ui.Context, outcome Outcome, key string, err error) {
	Toast(c, outcome, i18n.T(c, key))
	RecordFailure("desktop", key, outcome, err)
}

// RecordFailure logs the shown failure. A correlated Node failure stays local
// as a warning instead of opening a second automatic error report.
func RecordFailure(area, operation string, outcome Outcome, err error, attrs ...slog.Attr) {
	fault := failureFault(err)
	level := slog.LevelError
	if outcome != Failed || fault.Code == protocol.ErrorCodeCancelled || errors.Is(err, context.Canceled) || area == "desktop" && expectedRefusal(fault) {
		level = slog.LevelWarn
	}
	recordFailure(level, area, operation, fault, err, attrs)
}

// RecordWarning records an outcome without asking for another error report.
func RecordWarning(area, operation string, err error, attrs ...slog.Attr) {
	recordFailure(slog.LevelWarn, area, operation, failureFault(err), err, attrs)
}

func expectedRefusal(fault protocol.Fault) bool {
	switch fault.Code {
	case protocol.ErrorCodeConflict, protocol.ErrorCodeRevisionConflict, protocol.ErrorCodeOutcomeUnknown:
		return true
	case protocol.ErrorCodeNotConfigured:
		return fault.Details == nil || fault.ConfigurationReason().Expected()
	case protocol.ErrorCodeUnavailable:
		return fault.Details != nil && fault.Details.Kind == "network"
	}
	return false
}

func recordFailure(level slog.Level, area, operation string, fault protocol.Fault, err error, attrs []slog.Attr) {
	if fault.Code == "" && errors.Is(err, context.Canceled) {
		fault.Code = protocol.ErrorCodeCancelled
	}
	code := string(fault.Code)
	if fault.Code != protocol.ErrorCodeCancelled {
		if ref, owned := client.DeliveredReport(err); logging.ValidReportRef(ref) {
			if owned {
				level = slog.LevelWarn
			}
			attrs = append(attrs, slog.String("report_ref", ref))
		}
		if reason := fault.ConfigurationReason(); reason.Known() {
			attrs = append(attrs, slog.String("configuration_reason", string(reason)))
		}
		if area == "desktop" && operation == "inspect_git" {
			reason := fault.GitInspectionReason
			if stage := reason.Stage(); stage != "" {
				attrs = append(attrs, slog.String("stage", stage), slog.String("reason", string(reason)))
			}
		}
		if area == "desktop" && operation == "preferences_save_failed" {
			if stage, reason := preferences.WriteFailure(err); stage != "" {
				attrs = append(attrs, slog.String("stage", stage), slog.String("reason", reason))
			}
		}
	}
	if code == "" {
		code = "internal"
		if err == nil {
			code = "unspecified"
		}
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
