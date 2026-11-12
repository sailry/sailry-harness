// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
)

type ConversationBackgroundKind struct {
	Kind string
	Data any
}
type ConversationBackgroundKindAgentPayload struct {
	Session SessionID `json:"session"`
	Turn    TurnID    `json:"turn"`
}

func DefaultConversationBackgroundKindAgentPayload() ConversationBackgroundKindAgentPayload {
	return ConversationBackgroundKindAgentPayload{
		Session: SessionID{},
		Turn:    TurnID{},
	}
}
func (value *ConversationBackgroundKindAgentPayload) UnmarshalJSON(data []byte) error {
	type plain ConversationBackgroundKindAgentPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session", "turn"}, false); err != nil {
		return err
	}
	*value = ConversationBackgroundKindAgentPayload(decoded)
	return nil
}
func (value ConversationBackgroundKindAgentPayload) MarshalJSON() ([]byte, error) {
	type plain ConversationBackgroundKindAgentPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ConversationBackgroundKindCommandPayload struct {
	ID RequestID `json:"id"`
}

func DefaultConversationBackgroundKindCommandPayload() ConversationBackgroundKindCommandPayload {
	return ConversationBackgroundKindCommandPayload{
		ID: RequestID{},
	}
}
func (value *ConversationBackgroundKindCommandPayload) UnmarshalJSON(data []byte) error {
	type plain ConversationBackgroundKindCommandPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id"}, false); err != nil {
		return err
	}
	*value = ConversationBackgroundKindCommandPayload(decoded)
	return nil
}
func (value ConversationBackgroundKindCommandPayload) MarshalJSON() ([]byte, error) {
	type plain ConversationBackgroundKindCommandPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var conversationBackgroundKindVariants = variantFactories{"agent": func() any { return new(ConversationBackgroundKindAgentPayload) },
	"command": func() any { return new(ConversationBackgroundKindCommandPayload) }}

func (value ConversationBackgroundKind) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "data", value.Kind, value.Data, conversationBackgroundKindVariants)
}
func (value *ConversationBackgroundKind) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "data", conversationBackgroundKindVariants, false)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type ConversationBackgroundOutcome string

const (
	ConversationBackgroundOutcomeCompleted   ConversationBackgroundOutcome = "completed"
	ConversationBackgroundOutcomeFailed      ConversationBackgroundOutcome = "failed"
	ConversationBackgroundOutcomeCancelled   ConversationBackgroundOutcome = "cancelled"
	ConversationBackgroundOutcomeStopped     ConversationBackgroundOutcome = "stopped"
	ConversationBackgroundOutcomeTimedOut    ConversationBackgroundOutcome = "timed_out"
	ConversationBackgroundOutcomeInterrupted ConversationBackgroundOutcome = "interrupted"
	ConversationBackgroundOutcomeUnknown     ConversationBackgroundOutcome = "unknown"
	ConversationBackgroundOutcomeStalled     ConversationBackgroundOutcome = "stalled"
)

func (value ConversationBackgroundOutcome) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *ConversationBackgroundOutcome) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch ConversationBackgroundOutcome(decoded) {
	case ConversationBackgroundOutcomeCompleted, ConversationBackgroundOutcomeFailed, ConversationBackgroundOutcomeCancelled, ConversationBackgroundOutcomeStopped, ConversationBackgroundOutcomeTimedOut, ConversationBackgroundOutcomeInterrupted, ConversationBackgroundOutcomeUnknown, ConversationBackgroundOutcomeStalled:
		*value = ConversationBackgroundOutcome(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value ConversationBackgroundOutcome) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *ConversationBackgroundOutcome) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type ConversationBackgroundState string

const (
	ConversationBackgroundStateRunning   ConversationBackgroundState = "running"
	ConversationBackgroundStatePending   ConversationBackgroundState = "pending"
	ConversationBackgroundStateDelivered ConversationBackgroundState = "delivered"
	ConversationBackgroundStateConsumed  ConversationBackgroundState = "consumed"
	ConversationBackgroundStateDismissed ConversationBackgroundState = "dismissed"
)

func (value ConversationBackgroundState) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *ConversationBackgroundState) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch ConversationBackgroundState(decoded) {
	case ConversationBackgroundStateRunning, ConversationBackgroundStatePending, ConversationBackgroundStateDelivered, ConversationBackgroundStateConsumed, ConversationBackgroundStateDismissed:
		*value = ConversationBackgroundState(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value ConversationBackgroundState) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *ConversationBackgroundState) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

// ConversationBackgroundHold says why a pending, suppressed result starts no
// notice turn and waits for the user's next message.
type ConversationBackgroundHold string

const (
	ConversationBackgroundHoldLimit       ConversationBackgroundHold = "limit"
	ConversationBackgroundHoldStopped     ConversationBackgroundHold = "stopped"
	ConversationBackgroundHoldInterrupted ConversationBackgroundHold = "interrupted"
	ConversationBackgroundHoldFailed      ConversationBackgroundHold = "failed"
	ConversationBackgroundHoldOther       ConversationBackgroundHold = "other"
)

func (value ConversationBackgroundHold) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *ConversationBackgroundHold) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch ConversationBackgroundHold(decoded) {
	case ConversationBackgroundHoldLimit, ConversationBackgroundHoldStopped, ConversationBackgroundHoldInterrupted, ConversationBackgroundHoldFailed, ConversationBackgroundHoldOther:
		*value = ConversationBackgroundHold(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value ConversationBackgroundHold) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *ConversationBackgroundHold) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type ConversationBackgroundTask struct {
	ID         string                         `json:"id"`
	Session    SessionID                      `json:"session"`
	Turn       TurnID                         `json:"turn"`
	Kind       ConversationBackgroundKind     `json:"kind"`
	Title      string                         `json:"title"`
	Revision   uint64                         `json:"revision"`
	State      ConversationBackgroundState    `json:"state"`
	Outcome    *ConversationBackgroundOutcome `json:"outcome"`
	StartedMs  int64                          `json:"started_ms"`
	FinishedMs *int64                         `json:"finished_ms"`
	Summary    *string                        `json:"summary"`
	Delivery   *TurnID                        `json:"delivery"`
	Recovered  bool                           `json:"recovered"`
	Suppressed bool                           `json:"suppressed"`
	Hold       *ConversationBackgroundHold    `json:"hold,omitempty"`
	// ExitCode is the exit status of a command that ran to its end. It is
	// absent for an agent, a command that has not exited and a peer that
	// does not state it.
	ExitCode *int32 `json:"exit_code,omitempty"`
}

func DefaultConversationBackgroundTask() ConversationBackgroundTask {
	return ConversationBackgroundTask{
		ID:         "",
		Session:    SessionID{},
		Turn:       TurnID{},
		Kind:       ConversationBackgroundKind{},
		Title:      "",
		Revision:   0,
		State:      ConversationBackgroundState(""),
		Outcome:    nil,
		StartedMs:  0,
		FinishedMs: nil,
		Summary:    nil,
		Delivery:   nil,
		Recovered:  false,
		Suppressed: false,
		Hold:       nil,
		ExitCode:   nil,
	}
}
func (value *ConversationBackgroundTask) UnmarshalJSON(data []byte) error { return Decode(data, value) }
func (value *ConversationBackgroundTask) DecodeJSON(decoder *Decoder) error {
	type plain ConversationBackgroundTask
	decoded := plain{}
	if err := decoder.Object(&decoded, []string{"id", "session", "turn", "kind", "title", "revision", "state", "started_ms", "recovered", "suppressed"}, true); err != nil {
		return err
	}
	*value = ConversationBackgroundTask(decoded)
	return nil
}
func (value ConversationBackgroundTask) MarshalJSON() ([]byte, error) {
	type plain ConversationBackgroundTask
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ConversationBackgroundNotice struct {
	ID      string                        `json:"id"`
	Kind    ConversationBackgroundKind    `json:"kind"`
	Title   string                        `json:"title"`
	Outcome ConversationBackgroundOutcome `json:"outcome"`
	Summary *string                       `json:"summary"`
}

func DefaultConversationBackgroundNotice() ConversationBackgroundNotice {
	return ConversationBackgroundNotice{
		ID:      "",
		Kind:    ConversationBackgroundKind{},
		Title:   "",
		Outcome: ConversationBackgroundOutcome(""),
		Summary: nil,
	}
}
func (value *ConversationBackgroundNotice) UnmarshalJSON(data []byte) error {
	return Decode(data, value)
}
func (value *ConversationBackgroundNotice) DecodeJSON(decoder *Decoder) error {
	type plain ConversationBackgroundNotice
	decoded := plain{}
	if err := decoder.Object(&decoded, []string{"id", "kind", "title", "outcome"}, true); err != nil {
		return err
	}
	*value = ConversationBackgroundNotice(decoded)
	return nil
}
func (value ConversationBackgroundNotice) MarshalJSON() ([]byte, error) {
	type plain ConversationBackgroundNotice
	encoded := plain(value)
	return json.Marshal(encoded)
}
