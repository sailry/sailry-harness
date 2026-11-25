package node

import (
	"log/slog"

	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/logging"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func recordModelRequest(result model.RequestObservation) {
	level, phase := slog.LevelError, logging.Failed
	if result.Outcome == model.RequestRecovered {
		level, phase = slog.LevelWarn, logging.Completed
	}
	code := protocol.ErrorCodeUnavailable
	if result.Failure == model.NotConfigured {
		code = protocol.ErrorCodeNotConfigured
	}
	attrs := []slog.Attr{
		slog.String("area", "agent"), slog.String("operation", "model_request"), slog.String("phase", phase),
		slog.String("retry_outcome", string(result.Outcome)), slog.String("code", string(code)), slog.String("reason_code", "model_"+string(result.Failure)),
		slog.Uint64("attempts", result.Attempts),
	}
	if result.Status != 0 {
		attrs = append(attrs, slog.Uint64("http_status", uint64(result.Status)))
	}
	logging.Emit(level, attrs...)
}
