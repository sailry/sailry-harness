// Hand-maintained v1 protocol declarations; keep them consistent with the fixtures and tolerance tests.

package protocol

import (
	"encoding/json"
)

type ConversationCompactionSource string

const (
	ConversationCompactionSourceAutomatic ConversationCompactionSource = "automatic"
	ConversationCompactionSourceManual    ConversationCompactionSource = "manual"
)

func (value ConversationCompactionSource) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *ConversationCompactionSource) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch ConversationCompactionSource(decoded) {
	case ConversationCompactionSourceAutomatic, ConversationCompactionSourceManual:
		*value = ConversationCompactionSource(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value ConversationCompactionSource) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *ConversationCompactionSource) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type ConversationCompactionStage string

const (
	ConversationCompactionStageUnknown     ConversationCompactionStage = "unknown"
	ConversationCompactionStageHistory     ConversationCompactionStage = "history"
	ConversationCompactionStageBudget      ConversationCompactionStage = "budget"
	ConversationCompactionStageRequest     ConversationCompactionStage = "request"
	ConversationCompactionStageSummary     ConversationCompactionStage = "summary"
	ConversationCompactionStagePersistence ConversationCompactionStage = "persistence"
)

func (value ConversationCompactionStage) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *ConversationCompactionStage) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch ConversationCompactionStage(decoded) {
	case ConversationCompactionStageUnknown, ConversationCompactionStageHistory, ConversationCompactionStageBudget, ConversationCompactionStageRequest, ConversationCompactionStageSummary, ConversationCompactionStagePersistence:
		*value = ConversationCompactionStage(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value ConversationCompactionStage) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *ConversationCompactionStage) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type ConversationCompactionReason string

const (
	ConversationCompactionReasonUnknown             ConversationCompactionReason = "unknown"
	ConversationCompactionReasonHistoryMismatch     ConversationCompactionReason = "history_mismatch"
	ConversationCompactionReasonNoFittingPrefix     ConversationCompactionReason = "no_fitting_prefix"
	ConversationCompactionReasonRequestFailed       ConversationCompactionReason = "request_failed"
	ConversationCompactionReasonRequestTimeout      ConversationCompactionReason = "request_timeout"
	ConversationCompactionReasonRateLimited         ConversationCompactionReason = "rate_limited"
	ConversationCompactionReasonAuthorizationFailed ConversationCompactionReason = "authorization_failed"
	ConversationCompactionReasonServiceRejected     ConversationCompactionReason = "service_rejected"
	ConversationCompactionReasonContextOverflow     ConversationCompactionReason = "context_overflow"
	ConversationCompactionReasonContentFiltered     ConversationCompactionReason = "content_filtered"
	ConversationCompactionReasonEmptySummary        ConversationCompactionReason = "empty_summary"
	ConversationCompactionReasonIncompleteResponse  ConversationCompactionReason = "incomplete_response"
	ConversationCompactionReasonTruncatedSummary    ConversationCompactionReason = "truncated_summary"
	ConversationCompactionReasonUnexpectedOutput    ConversationCompactionReason = "unexpected_output"
	ConversationCompactionReasonSummaryTooLarge     ConversationCompactionReason = "summary_too_large"
	ConversationCompactionReasonNoReduction         ConversationCompactionReason = "no_reduction"
	ConversationCompactionReasonPersistenceFailed   ConversationCompactionReason = "persistence_failed"
)

func (value ConversationCompactionReason) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *ConversationCompactionReason) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch ConversationCompactionReason(decoded) {
	case ConversationCompactionReasonUnknown, ConversationCompactionReasonHistoryMismatch, ConversationCompactionReasonNoFittingPrefix, ConversationCompactionReasonRequestFailed, ConversationCompactionReasonRequestTimeout, ConversationCompactionReasonRateLimited, ConversationCompactionReasonAuthorizationFailed, ConversationCompactionReasonServiceRejected, ConversationCompactionReasonContextOverflow, ConversationCompactionReasonContentFiltered, ConversationCompactionReasonEmptySummary, ConversationCompactionReasonIncompleteResponse, ConversationCompactionReasonTruncatedSummary, ConversationCompactionReasonUnexpectedOutput, ConversationCompactionReasonSummaryTooLarge, ConversationCompactionReasonNoReduction, ConversationCompactionReasonPersistenceFailed:
		*value = ConversationCompactionReason(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value ConversationCompactionReason) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *ConversationCompactionReason) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type ConversationCompactionFailure struct {
	Source    ConversationCompactionSource `json:"source"`
	Stage     ConversationCompactionStage  `json:"stage"`
	Reason    ConversationCompactionReason `json:"reason"`
	Continued bool                         `json:"continued"`
	Status    *uint16                      `json:"status"`
	Cause     *string                      `json:"cause,omitempty"`
	// Paused says this failure paused automatic compaction for the session.
	Paused bool `json:"paused,omitempty"`
}

func DefaultConversationCompactionFailure() ConversationCompactionFailure {
	return ConversationCompactionFailure{
		Source:    ConversationCompactionSource(""),
		Stage:     ConversationCompactionStage(""),
		Reason:    ConversationCompactionReason(""),
		Continued: false,
		Status:    nil,
		Cause:     nil,
		Paused:    false,
	}
}
func (value *ConversationCompactionFailure) UnmarshalJSON(data []byte) error {
	type plain ConversationCompactionFailure
	decoded := plain{}
	decoded.Cause = nil
	decoded.Paused = false
	if err := DecodeObject(data, &decoded, []string{"source", "stage", "reason", "continued"}, true); err != nil {
		return err
	}
	*value = ConversationCompactionFailure(decoded)
	return nil
}
func (value ConversationCompactionFailure) MarshalJSON() ([]byte, error) {
	type plain ConversationCompactionFailure
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ConversationCompactionProgress struct {
	StartedMs int64 `json:"started_ms"`
}

func DefaultConversationCompactionProgress() ConversationCompactionProgress {
	return ConversationCompactionProgress{
		StartedMs: 0,
	}
}
func (value *ConversationCompactionProgress) UnmarshalJSON(data []byte) error {
	type plain ConversationCompactionProgress
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"started_ms"}, true); err != nil {
		return err
	}
	*value = ConversationCompactionProgress(decoded)
	return nil
}
func (value ConversationCompactionProgress) MarshalJSON() ([]byte, error) {
	type plain ConversationCompactionProgress
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ConversationCompactionBudget struct {
	Model         string `json:"model"`
	ContextLimit  uint32 `json:"context_limit"`
	TriggerTokens uint64 `json:"trigger_tokens"`
	OutputReserve uint32 `json:"output_reserve"`
	RecentTokens  uint64 `json:"recent_tokens"`
	SummaryInput  uint64 `json:"summary_input"`
	SummaryOutput uint32 `json:"summary_output"`
}

func DefaultConversationCompactionBudget() ConversationCompactionBudget {
	return ConversationCompactionBudget{
		Model:         "",
		ContextLimit:  0,
		TriggerTokens: 0,
		OutputReserve: 0,
		RecentTokens:  0,
		SummaryInput:  0,
		SummaryOutput: 0,
	}
}
func (value *ConversationCompactionBudget) UnmarshalJSON(data []byte) error {
	type plain ConversationCompactionBudget
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"model", "context_limit", "trigger_tokens", "output_reserve", "recent_tokens", "summary_input", "summary_output"}, false); err != nil {
		return err
	}
	*value = ConversationCompactionBudget(decoded)
	return nil
}
func (value ConversationCompactionBudget) MarshalJSON() ([]byte, error) {
	type plain ConversationCompactionBudget
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ConversationCompactionRecord struct {
	ID           string                        `json:"id"`
	Sequence     uint64                        `json:"sequence"`
	Turn         TurnID                        `json:"turn"`
	TurnSequence uint64                        `json:"turn_sequence"`
	TimestampMs  int64                         `json:"timestamp_ms"`
	Outcome      ConversationCompactionOutcome `json:"outcome"`
}

func DefaultConversationCompactionRecord() ConversationCompactionRecord {
	return ConversationCompactionRecord{
		ID:           "",
		Sequence:     0,
		Turn:         TurnID{},
		TurnSequence: 0,
		TimestampMs:  0,
		Outcome:      ConversationCompactionOutcome{},
	}
}
func (value *ConversationCompactionRecord) UnmarshalJSON(data []byte) error {
	type plain ConversationCompactionRecord
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id", "sequence", "turn", "turn_sequence", "timestamp_ms", "outcome"}, false); err != nil {
		return err
	}
	*value = ConversationCompactionRecord(decoded)
	return nil
}
func (value ConversationCompactionRecord) MarshalJSON() ([]byte, error) {
	type plain ConversationCompactionRecord
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ConversationCompactionOutcome struct {
	Kind string
	Data any
}

var conversationCompactionOutcomeVariants = variantFactories{"succeeded": nil,
	"failed": func() any { return new(ConversationCompactionFailure) }}

func (value ConversationCompactionOutcome) MarshalJSON() ([]byte, error) {
	return MarshalUnion("", "", value.Kind, value.Data, conversationCompactionOutcomeVariants)
}
func (value *ConversationCompactionOutcome) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "", "", conversationCompactionOutcomeVariants, false)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}
