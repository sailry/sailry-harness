package live

import (
	"strings"

	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// The i18n keys of what the Node reports without copy of its own: why a run
// failed, why a model retry stopped, a built-in tool's label and icon, and the
// kind of a tool failure that carries no message.

// stopKey is why the model recovery gave up.
func stopKey(reason protocol.ConversationRecoveryStop) string {
	switch reason {
	case protocol.ConversationRecoveryStopWaitExhausted:
		return "chat_model_retry_wait_exhausted"
	case protocol.ConversationRecoveryStopRateLimited:
		return "chat_model_rate_limited"
	case protocol.ConversationRecoveryStopServerOverloaded:
		return "chat_model_server_overloaded"
	case protocol.ConversationRecoveryStopInvalidRequest, protocol.ConversationRecoveryStopInvalidPrompt:
		return "chat_model_request_rejected"
	case protocol.ConversationRecoveryStopContextWindowExceeded:
		return "chat_model_context_exceeded"
	case protocol.ConversationRecoveryStopQuotaExceeded, protocol.ConversationRecoveryStopUsageLimitReached:
		return "chat_model_usage_exhausted"
	case protocol.ConversationRecoveryStopUsageNotIncluded:
		return "chat_model_usage_not_included"
	case protocol.ConversationRecoveryStopResponseIncomplete:
		return "chat_model_response_incomplete"
	case protocol.ConversationRecoveryStopFlexUnavailable:
		return "chat_model_flex_unavailable"
	case protocol.ConversationRecoveryStopPolicyViolation:
		return "chat_model_policy_rejected"
	case protocol.ConversationRecoveryStopLocalFailure:
		return "chat_model_request_unavailable"
	}
	return "chat_model_retry_exhausted"
}

// recoveryStop is the reason a run's failure gives for its stopped recovery.
func recoveryStop(fault *protocol.Fault) (protocol.ConversationRecoveryStop, bool) {
	if fault == nil || fault.Details == nil || fault.Details.Kind != "model_recovery" {
		return "", false
	}
	value, ok := detail[protocol.FaultDetailsModelRecoveryPayload](fault.Details.Data)
	return value.Reason, ok
}

// failureKey is the title of a failed run.
func failureKey(fault protocol.Fault) string {
	switch fault.Code {
	case protocol.ErrorCodeCancelled:
		return "turn_cancelled"
	case protocol.ErrorCodeOutcomeUnknown:
		return "chat_execution_unknown"
	}
	if reason, ok := recoveryStop(&fault); ok {
		return stopKey(reason)
	}
	if details := fault.Details; details != nil {
		switch details.Kind {
		case "system_diagnostic":
			if report, ok := detail[protocol.NotificationSystemSystemDiagnostic](details.Data); ok {
				switch report.Reason {
				case protocol.NotificationSystemDiagnosticReasonCancelled:
					return "turn_cancelled"
				case protocol.NotificationSystemDiagnosticReasonTimeout:
					return "plugin_response_timeout"
				}
				return "plugin_callback_failed"
			}
		case "model_request":
			if request, ok := detail[protocol.FaultDetailsModelRequestPayload](details.Data); ok {
				switch request.Reason {
				case protocol.ModelFailureRequest:
					return "chat_model_failed"
				case protocol.ModelFailureUnauthorized:
					return "chat_model_unauthorized"
				case protocol.ModelFailureRateLimited:
					return "chat_model_rate_limited"
				case protocol.ModelFailureTimeout:
					return "chat_response_timeout"
				case protocol.ModelFailureInvalidResponse:
					return "chat_response_invalid"
				case protocol.ModelFailureSharedUsageLimit:
					return "chat_subscription_limit"
				case protocol.ModelFailureSharedUsageUnavailable:
					return "chat_subscription_unavailable"
				}
			}
		}
	}
	switch fault.Message {
	case "context summary timed out":
		return "chat_compact_timeout"
	case "context summary did not reduce the conversation":
		return "chat_compact_no_reduction"
	case "context summary exceeds the size limit":
		return "chat_compact_too_large"
	case "context summary returned non-text output":
		return "chat_response_invalid"
	case "context summary was not completed", "context summary stream ended before completion":
		return "chat_compact_incomplete"
	}
	if fault.Code == protocol.ErrorCodeUnavailable {
		return "turn_failed"
	}
	return "chat_action_failed"
}

// coreTool is a built-in tool's label key and icon; "" for the rest, whose
// declaration or captured title names them.
func coreTool(name string) (label, icon string) {
	switch name {
	case "set_session_title":
		return "tool_session_title", ""
	case "load_skill":
		return "tool_load_skill", "book-open"
	case "read_skill_resource":
		return "tool_read_skill_resource", "book-open"
	case "list_sessions":
		return "tool_list_sessions", "bot"
	case "read_session":
		return "tool_read_session", "file-text"
	case "search_session":
		return "tool_search_session", "search"
	case "tool_search":
		return "tool_find_tools", "search"
	case "read_tool_output":
		return "tool_read_output", "file-text"
	case "compact_context":
		return "chat_compact", "file-text"
	case "plan_review":
		return "composer_mode_plan", ""
	}
	return "", ""
}

// toolFailure is a failed call's line: the tool's own message, or the kind of
// the failure in the user's language when it gave none.
func toolFailure(language string, summary *protocol.ToolDetailSummary) string {
	if summary.Error != nil && strings.TrimSpace(*summary.Error) != "" {
		return *summary.Error
	}
	if summary.Failure == nil {
		return ""
	}
	key := "tool_failed"
	if code := summary.Failure.Code; code != nil {
		switch *code {
		case protocol.ErrorCodeNotFound:
			key = "tool_not_found"
		case protocol.ErrorCodeInvalidRequest:
			key = "tool_invalid"
		case protocol.ErrorCodePermissionDenied:
			key = "tool_denied"
		case protocol.ErrorCodeCancelled:
			key = "turn_cancelled"
		case protocol.ErrorCodeOutcomeUnknown:
			key = "tool_interrupted"
		case protocol.ErrorCodeUnavailable:
			key = "tool_unavailable"
		}
	}
	return i18n.In(language, key)
}
