// Hand-maintained v1 protocol declarations; keep them consistent with the fixtures and tolerance tests.

package protocol

import (
	"encoding/json"
)

type NotificationDetails struct {
	Kind string
	Data any
}
type NotificationDetailsTextPayload struct {
	Body string `json:"body"`
}

func DefaultNotificationDetailsTextPayload() NotificationDetailsTextPayload {
	return NotificationDetailsTextPayload{
		Body: "",
	}
}
func (value *NotificationDetailsTextPayload) UnmarshalJSON(data []byte) error {
	type plain NotificationDetailsTextPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"body"}, false); err != nil {
		return err
	}
	*value = NotificationDetailsTextPayload(decoded)
	return nil
}
func (value NotificationDetailsTextPayload) MarshalJSON() ([]byte, error) {
	type plain NotificationDetailsTextPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type NotificationDetailsPowerPayload struct {
	Code int64 `json:"code"`
}

func DefaultNotificationDetailsPowerPayload() NotificationDetailsPowerPayload {
	return NotificationDetailsPowerPayload{
		Code: 0,
	}
}
func (value *NotificationDetailsPowerPayload) UnmarshalJSON(data []byte) error {
	type plain NotificationDetailsPowerPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"code"}, false); err != nil {
		return err
	}
	*value = NotificationDetailsPowerPayload(decoded)
	return nil
}
func (value NotificationDetailsPowerPayload) MarshalJSON() ([]byte, error) {
	type plain NotificationDetailsPowerPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var notificationDetailsVariants = variantFactories{"diagnostic": func() any { return new(NotificationDetailsDiagnostic) },
	"system":   func() any { return new(NotificationSystemSystemDiagnostic) },
	"task":     func() any { return new(NotificationDetailsTask) },
	"terminal": func() any { return new(NotificationDetailsTerminal) },
	"text":     func() any { return new(NotificationDetailsTextPayload) },
	"power":    func() any { return new(NotificationDetailsPowerPayload) }}

func (value NotificationDetails) MarshalJSON() ([]byte, error) {
	return MarshalUnion("type", "data", value.Kind, value.Data, notificationDetailsVariants)
}
func (value *NotificationDetails) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "type", "data", notificationDetailsVariants, false)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type NotificationDetailsDiagnostic struct {
	Event    string                   `json:"event"`
	Sequence uint64                   `json:"sequence"`
	Turn     TurnID                   `json:"turn"`
	Cause    NotificationDetailsCause `json:"cause"`
	Version  string                   `json:"version"`
	Platform string                   `json:"platform"`
}

func DefaultNotificationDetailsDiagnostic() NotificationDetailsDiagnostic {
	return NotificationDetailsDiagnostic{
		Event:    "",
		Sequence: 0,
		Turn:     TurnID{},
		Cause:    NotificationDetailsCause{},
		Version:  "",
		Platform: "",
	}
}
func (value *NotificationDetailsDiagnostic) UnmarshalJSON(data []byte) error {
	type plain NotificationDetailsDiagnostic
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"event", "sequence", "turn", "cause", "version", "platform"}, true); err != nil {
		return err
	}
	*value = NotificationDetailsDiagnostic(decoded)
	return nil
}
func (value NotificationDetailsDiagnostic) MarshalJSON() ([]byte, error) {
	type plain NotificationDetailsDiagnostic
	encoded := plain(value)
	return json.Marshal(encoded)
}

type NotificationDetailsCause struct {
	Kind string
	Data any
}
type NotificationDetailsCauseExecutionPayload struct {
	Code ErrorCode `json:"code"`
}

func DefaultNotificationDetailsCauseExecutionPayload() NotificationDetailsCauseExecutionPayload {
	return NotificationDetailsCauseExecutionPayload{
		Code: ErrorCode(""),
	}
}
func (value *NotificationDetailsCauseExecutionPayload) UnmarshalJSON(data []byte) error {
	type plain NotificationDetailsCauseExecutionPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"code"}, false); err != nil {
		return err
	}
	*value = NotificationDetailsCauseExecutionPayload(decoded)
	return nil
}
func (value NotificationDetailsCauseExecutionPayload) MarshalJSON() ([]byte, error) {
	type plain NotificationDetailsCauseExecutionPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var notificationDetailsCauseVariants = variantFactories{"compaction": func() any { return new(ConversationCompactionFailure) },
	"execution": func() any { return new(NotificationDetailsCauseExecutionPayload) }}

func (value NotificationDetailsCause) MarshalJSON() ([]byte, error) {
	return MarshalUnion("type", "data", value.Kind, value.Data, notificationDetailsCauseVariants)
}
func (value *NotificationDetailsCause) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "type", "data", notificationDetailsCauseVariants, false)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type NotificationDetailsTask struct {
	Turn       TurnID             `json:"turn"`
	Status     ConversationStatus `json:"status"`
	StartedMs  *int64             `json:"started_ms"`
	FinishedMs *int64             `json:"finished_ms"`
	Error      *ErrorCode         `json:"error"`
}

func DefaultNotificationDetailsTask() NotificationDetailsTask {
	return NotificationDetailsTask{
		Turn:       TurnID{},
		Status:     ConversationStatus(""),
		StartedMs:  nil,
		FinishedMs: nil,
		Error:      nil,
	}
}
func (value *NotificationDetailsTask) UnmarshalJSON(data []byte) error {
	type plain NotificationDetailsTask
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"turn", "status"}, true); err != nil {
		return err
	}
	*value = NotificationDetailsTask(decoded)
	return nil
}
func (value NotificationDetailsTask) MarshalJSON() ([]byte, error) {
	type plain NotificationDetailsTask
	encoded := plain(value)
	return json.Marshal(encoded)
}

type NotificationDetailsTerminal struct {
	ID     TerminalID `json:"id"`
	Code   *uint32    `json:"code"`
	Failed bool       `json:"failed"`
}

func DefaultNotificationDetailsTerminal() NotificationDetailsTerminal {
	return NotificationDetailsTerminal{
		ID:     TerminalID{},
		Code:   nil,
		Failed: false,
	}
}
func (value *NotificationDetailsTerminal) UnmarshalJSON(data []byte) error {
	type plain NotificationDetailsTerminal
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id", "failed"}, true); err != nil {
		return err
	}
	*value = NotificationDetailsTerminal(decoded)
	return nil
}
func (value NotificationDetailsTerminal) MarshalJSON() ([]byte, error) {
	type plain NotificationDetailsTerminal
	encoded := plain(value)
	return json.Marshal(encoded)
}
