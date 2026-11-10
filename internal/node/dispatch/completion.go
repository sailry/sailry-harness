package dispatch

import "github.com/sailry/sailry-harness/internal/protocol"

func TurnResult(run protocol.ConversationRun) (bool, error) {
	var failure *protocol.Fault
	switch run.Status {
	case protocol.ConversationStatusQueued, protocol.ConversationStatusRunning, protocol.ConversationStatusStopping:
		return false, nil
	case protocol.ConversationStatusCompleted:
		return true, nil
	case protocol.ConversationStatusContinued:
		failure = &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "task turn continued in another invocation"}
	case protocol.ConversationStatusCancelled:
		failure = &protocol.Fault{Code: protocol.ErrorCodeCancelled, Message: "task turn was cancelled"}
	case protocol.ConversationStatusInterrupted:
		failure = &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "task turn was interrupted"}
	default:
		failure = &protocol.Fault{Code: protocol.ErrorCodeInternal, Message: "task turn failed"}
	}
	if run.Error != nil {
		failure = run.Error
	}
	return true, failure
}
