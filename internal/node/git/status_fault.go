package git

import (
	"errors"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func statusFault(code protocol.ErrorCode, message string, reason protocol.GitInspectionReason) error {
	return &protocol.Fault{Code: code, Message: message, GitInspectionReason: reason}
}

func statusFailure(err error, reason protocol.GitInspectionReason) error {
	var original *protocol.Fault
	if !errors.As(err, &original) || original == nil || original.Code == protocol.ErrorCodeCancelled {
		return err
	}
	annotated := *original
	annotated.GitInspectionReason = reason
	return &annotated
}

func statusCommandFailure(err error, result CommandResult) error {
	if result.ExitCode != 0 {
		return statusFailure(err, protocol.GitInspectionStatusNonzeroExit)
	}
	return statusFailure(err, protocol.GitInspectionStatusCommandFailed)
}
