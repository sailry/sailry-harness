// Hand-maintained v1 protocol declarations; keep them consistent with the fixtures and tolerance tests.

package protocol

import (
	"encoding/json"
)

type ConversationRecoveryTransport string

const (
	ConversationRecoveryTransportHttp ConversationRecoveryTransport = "http"
)

func (value ConversationRecoveryTransport) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *ConversationRecoveryTransport) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch ConversationRecoveryTransport(decoded) {
	case ConversationRecoveryTransportHttp:
		*value = ConversationRecoveryTransport(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value ConversationRecoveryTransport) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *ConversationRecoveryTransport) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type ConversationRecoveryStop string

const (
	ConversationRecoveryStopAttemptsExhausted     ConversationRecoveryStop = "attempts_exhausted"
	ConversationRecoveryStopRateLimited           ConversationRecoveryStop = "rate_limited"
	ConversationRecoveryStopServerOverloaded      ConversationRecoveryStop = "server_overloaded"
	ConversationRecoveryStopInvalidRequest        ConversationRecoveryStop = "invalid_request"
	ConversationRecoveryStopInvalidPrompt         ConversationRecoveryStop = "invalid_prompt"
	ConversationRecoveryStopContextWindowExceeded ConversationRecoveryStop = "context_window_exceeded"
	ConversationRecoveryStopQuotaExceeded         ConversationRecoveryStop = "quota_exceeded"
	ConversationRecoveryStopUsageNotIncluded      ConversationRecoveryStop = "usage_not_included"
	ConversationRecoveryStopUsageLimitReached     ConversationRecoveryStop = "usage_limit_reached"
	ConversationRecoveryStopResponseIncomplete    ConversationRecoveryStop = "response_incomplete"
	ConversationRecoveryStopFlexUnavailable       ConversationRecoveryStop = "flex_unavailable"
	ConversationRecoveryStopPolicyViolation       ConversationRecoveryStop = "policy_violation"
	ConversationRecoveryStopLocalFailure          ConversationRecoveryStop = "local_failure"
)

func (value ConversationRecoveryStop) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *ConversationRecoveryStop) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch ConversationRecoveryStop(decoded) {
	case ConversationRecoveryStopAttemptsExhausted, ConversationRecoveryStopRateLimited, ConversationRecoveryStopServerOverloaded, ConversationRecoveryStopInvalidRequest, ConversationRecoveryStopInvalidPrompt, ConversationRecoveryStopContextWindowExceeded, ConversationRecoveryStopQuotaExceeded, ConversationRecoveryStopUsageNotIncluded, ConversationRecoveryStopUsageLimitReached, ConversationRecoveryStopResponseIncomplete, ConversationRecoveryStopFlexUnavailable, ConversationRecoveryStopPolicyViolation, ConversationRecoveryStopLocalFailure:
		*value = ConversationRecoveryStop(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value ConversationRecoveryStop) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *ConversationRecoveryStop) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type ConversationRecoveryRetry struct {
	Attempt   uint64                         `json:"attempt"`
	Fallback  bool                           `json:"fallback,omitempty"`
	Limit     *uint64                        `json:"limit"`
	DelayMs   *uint64                        `json:"delay_ms,omitempty"`
	Transport *ConversationRecoveryTransport `json:"transport,omitempty"`
	Status    *uint16                        `json:"status,omitempty"`
	Code      *string                        `json:"code,omitempty"`
	Error     *string                        `json:"error,omitempty"`
}

func DefaultConversationRecoveryRetry() ConversationRecoveryRetry {
	return ConversationRecoveryRetry{
		Attempt:   0,
		Fallback:  false,
		Limit:     nil,
		DelayMs:   nil,
		Transport: nil,
		Status:    nil,
		Code:      nil,
		Error:     nil,
	}
}
func (value *ConversationRecoveryRetry) UnmarshalJSON(data []byte) error {
	type plain ConversationRecoveryRetry
	decoded := plain{}
	decoded.Fallback = false
	decoded.DelayMs = nil
	decoded.Transport = nil
	decoded.Status = nil
	decoded.Code = nil
	decoded.Error = nil
	if err := DecodeObject(data, &decoded, []string{"attempt"}, false); err != nil {
		return err
	}
	*value = ConversationRecoveryRetry(decoded)
	return nil
}
func (value ConversationRecoveryRetry) MarshalJSON() ([]byte, error) {
	type plain ConversationRecoveryRetry
	encoded := plain(value)
	return json.Marshal(encoded)
}
