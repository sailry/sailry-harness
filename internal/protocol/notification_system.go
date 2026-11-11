// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
)

type NotificationSystemSystemSource string

const (
	NotificationSystemSystemSourcePluginMetadata       NotificationSystemSystemSource = "plugin_metadata"
	NotificationSystemSystemSourcePluginInitialization NotificationSystemSystemSource = "plugin_initialization"
	NotificationSystemSystemSourceGitInspection        NotificationSystemSystemSource = "git_inspection"
	NotificationSystemSystemSourcePluginCallback       NotificationSystemSystemSource = "plugin_callback"
)

func (value NotificationSystemSystemSource) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *NotificationSystemSystemSource) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch NotificationSystemSystemSource(decoded) {
	case NotificationSystemSystemSourcePluginMetadata, NotificationSystemSystemSourcePluginInitialization, NotificationSystemSystemSourceGitInspection, NotificationSystemSystemSourcePluginCallback:
		*value = NotificationSystemSystemSource(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value NotificationSystemSystemSource) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *NotificationSystemSystemSource) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type NotificationSystemDiagnosticPhase string

const (
	NotificationSystemDiagnosticPhaseMetadata       NotificationSystemDiagnosticPhase = "metadata"
	NotificationSystemDiagnosticPhaseInitialization NotificationSystemDiagnosticPhase = "initialization"
	NotificationSystemDiagnosticPhaseInspection     NotificationSystemDiagnosticPhase = "inspection"
	NotificationSystemDiagnosticPhaseCallback       NotificationSystemDiagnosticPhase = "callback"
	NotificationSystemDiagnosticPhaseSdk            NotificationSystemDiagnosticPhase = "sdk"
)

func (value NotificationSystemDiagnosticPhase) MarshalJSON() ([]byte, error) {
	return marshalEnum(value)
}
func (value *NotificationSystemDiagnosticPhase) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch NotificationSystemDiagnosticPhase(decoded) {
	case NotificationSystemDiagnosticPhaseMetadata, NotificationSystemDiagnosticPhaseInitialization, NotificationSystemDiagnosticPhaseInspection, NotificationSystemDiagnosticPhaseCallback, NotificationSystemDiagnosticPhaseSdk:
		*value = NotificationSystemDiagnosticPhase(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value NotificationSystemDiagnosticPhase) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *NotificationSystemDiagnosticPhase) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type NotificationSystemDiagnosticReason string

const (
	NotificationSystemDiagnosticReasonFailure   NotificationSystemDiagnosticReason = "failure"
	NotificationSystemDiagnosticReasonTimeout   NotificationSystemDiagnosticReason = "timeout"
	NotificationSystemDiagnosticReasonCancelled NotificationSystemDiagnosticReason = "cancelled"
)

func (value NotificationSystemDiagnosticReason) MarshalJSON() ([]byte, error) {
	return marshalEnum(value)
}
func (value *NotificationSystemDiagnosticReason) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch NotificationSystemDiagnosticReason(decoded) {
	case NotificationSystemDiagnosticReasonFailure, NotificationSystemDiagnosticReasonTimeout, NotificationSystemDiagnosticReasonCancelled:
		*value = NotificationSystemDiagnosticReason(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value NotificationSystemDiagnosticReason) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *NotificationSystemDiagnosticReason) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type NotificationSystemDiagnosticCause string

const (
	NotificationSystemDiagnosticCauseGitQueueTimeout NotificationSystemDiagnosticCause = "git_queue_timeout"
	NotificationSystemDiagnosticCauseGitReadTimeout  NotificationSystemDiagnosticCause = "git_read_timeout"
	NotificationSystemDiagnosticCauseGitLocked       NotificationSystemDiagnosticCause = "git_locked"
)

func (value NotificationSystemDiagnosticCause) MarshalJSON() ([]byte, error) {
	return marshalEnum(value)
}
func (value *NotificationSystemDiagnosticCause) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch NotificationSystemDiagnosticCause(decoded) {
	case NotificationSystemDiagnosticCauseGitQueueTimeout, NotificationSystemDiagnosticCauseGitReadTimeout, NotificationSystemDiagnosticCauseGitLocked:
		*value = NotificationSystemDiagnosticCause(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value NotificationSystemDiagnosticCause) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *NotificationSystemDiagnosticCause) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type NotificationSystemScriptException string

const (
	NotificationSystemScriptExceptionError          NotificationSystemScriptException = "error"
	NotificationSystemScriptExceptionTypeError      NotificationSystemScriptException = "type_error"
	NotificationSystemScriptExceptionRangeError     NotificationSystemScriptException = "range_error"
	NotificationSystemScriptExceptionReferenceError NotificationSystemScriptException = "reference_error"
	NotificationSystemScriptExceptionSyntaxError    NotificationSystemScriptException = "syntax_error"
	NotificationSystemScriptExceptionUriError       NotificationSystemScriptException = "uri_error"
	NotificationSystemScriptExceptionEvalError      NotificationSystemScriptException = "eval_error"
	NotificationSystemScriptExceptionAggregateError NotificationSystemScriptException = "aggregate_error"
	NotificationSystemScriptExceptionThrownValue    NotificationSystemScriptException = "thrown_value"
	NotificationSystemScriptExceptionUnknown        NotificationSystemScriptException = "unknown"
)

func (value NotificationSystemScriptException) MarshalJSON() ([]byte, error) {
	return marshalEnum(value)
}
func (value *NotificationSystemScriptException) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch NotificationSystemScriptException(decoded) {
	case NotificationSystemScriptExceptionError, NotificationSystemScriptExceptionTypeError, NotificationSystemScriptExceptionRangeError, NotificationSystemScriptExceptionReferenceError, NotificationSystemScriptExceptionSyntaxError, NotificationSystemScriptExceptionUriError, NotificationSystemScriptExceptionEvalError, NotificationSystemScriptExceptionAggregateError, NotificationSystemScriptExceptionThrownValue, NotificationSystemScriptExceptionUnknown:
		*value = NotificationSystemScriptException(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value NotificationSystemScriptException) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *NotificationSystemScriptException) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type NotificationSystemDiagnosticPlugin struct {
	Name    string `json:"name"`
	Version string `json:"version"`
}

func DefaultNotificationSystemDiagnosticPlugin() NotificationSystemDiagnosticPlugin {
	return NotificationSystemDiagnosticPlugin{
		Name:    "",
		Version: "",
	}
}
func (value *NotificationSystemDiagnosticPlugin) UnmarshalJSON(data []byte) error {
	type plain NotificationSystemDiagnosticPlugin
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"name", "version"}, true); err != nil {
		return err
	}
	*value = NotificationSystemDiagnosticPlugin(decoded)
	return nil
}
func (value NotificationSystemDiagnosticPlugin) MarshalJSON() ([]byte, error) {
	type plain NotificationSystemDiagnosticPlugin
	encoded := plain(value)
	return json.Marshal(encoded)
}

type NotificationSystemSystemDiagnostic struct {
	Incident  RequestID                           `json:"incident"`
	Source    NotificationSystemSystemSource      `json:"source"`
	Phase     NotificationSystemDiagnosticPhase   `json:"phase"`
	Reason    NotificationSystemDiagnosticReason  `json:"reason"`
	Code      ErrorCode                           `json:"code"`
	Cause     *NotificationSystemDiagnosticCause  `json:"cause,omitempty"`
	Exception *NotificationSystemScriptException  `json:"exception"`
	Plugin    *NotificationSystemDiagnosticPlugin `json:"plugin"`
	Request   *RequestID                          `json:"request"`
	Worktree  *WorktreeID                         `json:"worktree"`
	Session   *SessionID                          `json:"session"`
	Turn      *TurnID                             `json:"turn"`
	Operation *string                             `json:"operation"`
	BudgetMs  *uint64                             `json:"budget_ms"`
	ElapsedMs *uint64                             `json:"elapsed_ms"`
	Version   string                              `json:"version"`
	Platform  string                              `json:"platform"`
}

func DefaultNotificationSystemSystemDiagnostic() NotificationSystemSystemDiagnostic {
	return NotificationSystemSystemDiagnostic{
		Incident:  RequestID{},
		Source:    NotificationSystemSystemSource(""),
		Phase:     NotificationSystemDiagnosticPhase(""),
		Reason:    NotificationSystemDiagnosticReason(""),
		Code:      ErrorCode(""),
		Cause:     nil,
		Exception: nil,
		Plugin:    nil,
		Request:   nil,
		Worktree:  nil,
		Session:   nil,
		Turn:      nil,
		Operation: nil,
		BudgetMs:  nil,
		ElapsedMs: nil,
		Version:   "",
		Platform:  "",
	}
}
func (value *NotificationSystemSystemDiagnostic) UnmarshalJSON(data []byte) error {
	type plain NotificationSystemSystemDiagnostic
	decoded := plain{}
	decoded.Cause = nil
	decoded.Exception = nil
	if err := DecodeObject(data, &decoded, []string{"incident", "source", "phase", "reason", "code", "version", "platform"}, true); err != nil {
		return err
	}
	*value = NotificationSystemSystemDiagnostic(decoded)
	return nil
}
func (value NotificationSystemSystemDiagnostic) MarshalJSON() ([]byte, error) {
	type plain NotificationSystemSystemDiagnostic
	encoded := plain(value)
	return json.Marshal(encoded)
}
