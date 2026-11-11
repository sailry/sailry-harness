// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
)

type ConversationObservationSummary struct {
	Recent        []ConversationObservationRequest  `json:"recent"`
	Latency       ConversationObservationLatency    `json:"latency"`
	Phases        ConversationObservationPhases     `json:"phases"`
	Calls         uint64                            `json:"calls"`
	Replies       uint64                            `json:"replies"`
	Compactions   uint64                            `json:"compactions"`
	Tools         map[string]uint64                 `json:"tools"`
	Children      uint64                            `json:"children"`
	CacheRead     *uint64                           `json:"cache_read"`
	Reasoning     *uint64                           `json:"reasoning"`
	ModelUs       uint64                            `json:"model_us"`
	OutputUs      uint64                            `json:"output_us"`
	OutputTokens  uint64                            `json:"output_tokens"`
	OutputSamples uint64                            `json:"output_samples"`
	Composition   []ConversationObservationCategory `json:"composition"`
}

func DefaultConversationObservationSummary() ConversationObservationSummary {
	return ConversationObservationSummary{
		Recent:        []ConversationObservationRequest{},
		Latency:       DefaultConversationObservationLatency(),
		Phases:        DefaultConversationObservationPhases(),
		Calls:         0,
		Replies:       0,
		Compactions:   0,
		Tools:         map[string]uint64{},
		Children:      0,
		CacheRead:     nil,
		Reasoning:     nil,
		ModelUs:       0,
		OutputUs:      0,
		OutputTokens:  0,
		OutputSamples: 0,
		Composition:   []ConversationObservationCategory{},
	}
}
func (value *ConversationObservationSummary) UnmarshalJSON(data []byte) error {
	type plain ConversationObservationSummary
	decoded := plain{}
	decoded.Latency = DefaultConversationObservationLatency()
	decoded.Phases = DefaultConversationObservationPhases()
	if err := DecodeObject(data, &decoded, []string{"recent", "calls", "replies", "compactions", "tools", "children", "model_us", "output_us", "output_tokens", "output_samples", "composition"}, false); err != nil {
		return err
	}
	*value = ConversationObservationSummary(decoded)
	return nil
}
func (value ConversationObservationSummary) MarshalJSON() ([]byte, error) {
	type plain ConversationObservationSummary
	encoded := plain(value)
	encoded.Recent = normalizeCollections(encoded.Recent)
	encoded.Tools = normalizeCollections(encoded.Tools)
	encoded.Composition = normalizeCollections(encoded.Composition)
	return json.Marshal(encoded)
}

type ConversationObservationLatency struct {
	ResponseUs   *uint64 `json:"response_us"`
	FirstTokenUs *uint64 `json:"first_token_us"`
	WaitingUs    *uint64 `json:"waiting_us"`
	ElapsedUs    *uint64 `json:"elapsed_us"`
}

func DefaultConversationObservationLatency() ConversationObservationLatency {
	return ConversationObservationLatency{
		ResponseUs:   nil,
		FirstTokenUs: nil,
		WaitingUs:    nil,
		ElapsedUs:    nil,
	}
}
func (value *ConversationObservationLatency) UnmarshalJSON(data []byte) error {
	type plain ConversationObservationLatency
	decoded := plain{}
	decoded.ResponseUs = nil
	decoded.FirstTokenUs = nil
	decoded.WaitingUs = nil
	decoded.ElapsedUs = nil
	if err := DecodeObject(data, &decoded, []string{}, false); err != nil {
		return err
	}
	*value = ConversationObservationLatency(decoded)
	return nil
}
func (value ConversationObservationLatency) MarshalJSON() ([]byte, error) {
	type plain ConversationObservationLatency
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ConversationObservationRequest struct {
	ID           string                           `json:"id"`
	Turn         TurnID                           `json:"turn"`
	TurnIndex    *uint64                          `json:"turn_index,omitempty"`
	Model        string                           `json:"model"`
	ContextLimit *uint32                          `json:"context_limit"`
	TimestampMs  int64                            `json:"timestamp_ms"`
	Boundary     *uint64                          `json:"boundary"`
	Attempts     []ConversationObservationAttempt `json:"attempts"`
}

func DefaultConversationObservationRequest() ConversationObservationRequest {
	return ConversationObservationRequest{
		ID:           "",
		Turn:         TurnID{},
		TurnIndex:    nil,
		Model:        "",
		ContextLimit: nil,
		TimestampMs:  0,
		Boundary:     nil,
		Attempts:     []ConversationObservationAttempt{},
	}
}
func (value *ConversationObservationRequest) UnmarshalJSON(data []byte) error {
	type plain ConversationObservationRequest
	decoded := plain{}
	decoded.TurnIndex = nil
	if err := DecodeObject(data, &decoded, []string{"id", "turn", "model", "timestamp_ms", "attempts"}, false); err != nil {
		return err
	}
	*value = ConversationObservationRequest(decoded)
	return nil
}
func (value ConversationObservationRequest) MarshalJSON() ([]byte, error) {
	type plain ConversationObservationRequest
	encoded := plain(value)
	encoded.Attempts = normalizeCollections(encoded.Attempts)
	return json.Marshal(encoded)
}

type ConversationObservationAttempt struct {
	Index             uint64                            `json:"index"`
	Transport         string                            `json:"transport"`
	Reused            *bool                             `json:"reused"`
	HeadersUs         *uint64                           `json:"headers_us"`
	ResponseUs        *uint64                           `json:"response_us"`
	FirstTokenUs      *uint64                           `json:"first_token_us"`
	ElapsedUs         *uint64                           `json:"elapsed_us"`
	Status            *uint16                           `json:"status"`
	Outcome           ConversationObservationOutcome    `json:"outcome"`
	Categories        []ConversationObservationCategory `json:"categories"`
	CompleteContext   bool                              `json:"complete_context"`
	Usage             *ConversationUsage                `json:"usage"`
	CacheReported     bool                              `json:"cache_reported"`
	ReasoningReported bool                              `json:"reasoning_reported"`
	ReasoningStreamed bool                              `json:"reasoning_streamed"`
}

func DefaultConversationObservationAttempt() ConversationObservationAttempt {
	return ConversationObservationAttempt{
		Index:             0,
		Transport:         "",
		Reused:            nil,
		HeadersUs:         nil,
		ResponseUs:        nil,
		FirstTokenUs:      nil,
		ElapsedUs:         nil,
		Status:            nil,
		Outcome:           ConversationObservationOutcome(""),
		Categories:        []ConversationObservationCategory{},
		CompleteContext:   false,
		Usage:             nil,
		CacheReported:     false,
		ReasoningReported: false,
		ReasoningStreamed: false,
	}
}
func (value *ConversationObservationAttempt) UnmarshalJSON(data []byte) error {
	type plain ConversationObservationAttempt
	decoded := plain{}
	decoded.ResponseUs = nil
	decoded.ReasoningStreamed = false
	if err := DecodeObject(data, &decoded, []string{"index", "transport", "outcome", "categories", "complete_context", "cache_reported", "reasoning_reported"}, false); err != nil {
		return err
	}
	*value = ConversationObservationAttempt(decoded)
	return nil
}
func (value ConversationObservationAttempt) MarshalJSON() ([]byte, error) {
	type plain ConversationObservationAttempt
	encoded := plain(value)
	encoded.Categories = normalizeCollections(encoded.Categories)
	return json.Marshal(encoded)
}

type ConversationObservationOutcome string

const (
	ConversationObservationOutcomeSucceeded ConversationObservationOutcome = "succeeded"
	ConversationObservationOutcomeFailed    ConversationObservationOutcome = "failed"
	ConversationObservationOutcomeCancelled ConversationObservationOutcome = "cancelled"
)

func (value ConversationObservationOutcome) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *ConversationObservationOutcome) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch ConversationObservationOutcome(decoded) {
	case ConversationObservationOutcomeSucceeded, ConversationObservationOutcomeFailed, ConversationObservationOutcomeCancelled:
		*value = ConversationObservationOutcome(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value ConversationObservationOutcome) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *ConversationObservationOutcome) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type ConversationObservationCategory struct {
	Kind   ConversationObservationCategoryKind `json:"kind"`
	Items  *uint64                             `json:"items"`
	Tokens *uint64                             `json:"tokens"`
	Method *string                             `json:"method"`
}

func DefaultConversationObservationCategory() ConversationObservationCategory {
	return ConversationObservationCategory{
		Kind:   ConversationObservationCategoryKind(""),
		Items:  nil,
		Tokens: nil,
		Method: nil,
	}
}
func (value *ConversationObservationCategory) UnmarshalJSON(data []byte) error {
	type plain ConversationObservationCategory
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"kind"}, false); err != nil {
		return err
	}
	*value = ConversationObservationCategory(decoded)
	return nil
}
func (value ConversationObservationCategory) MarshalJSON() ([]byte, error) {
	type plain ConversationObservationCategory
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ConversationObservationCategoryKind string

const (
	ConversationObservationCategoryKindSystem    ConversationObservationCategoryKind = "system"
	ConversationObservationCategoryKindTools     ConversationObservationCategoryKind = "tools"
	ConversationObservationCategoryKindUser      ConversationObservationCategoryKind = "user"
	ConversationObservationCategoryKindSkills    ConversationObservationCategoryKind = "skills"
	ConversationObservationCategoryKindInjected  ConversationObservationCategoryKind = "injected"
	ConversationObservationCategoryKindAssistant ConversationObservationCategoryKind = "assistant"
	ConversationObservationCategoryKindResults   ConversationObservationCategoryKind = "results"
)

func (value ConversationObservationCategoryKind) MarshalJSON() ([]byte, error) {
	return marshalEnum(value)
}
func (value *ConversationObservationCategoryKind) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch ConversationObservationCategoryKind(decoded) {
	case ConversationObservationCategoryKindSystem, ConversationObservationCategoryKindTools, ConversationObservationCategoryKindUser, ConversationObservationCategoryKindSkills, ConversationObservationCategoryKindInjected, ConversationObservationCategoryKindAssistant, ConversationObservationCategoryKindResults:
		*value = ConversationObservationCategoryKind(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value ConversationObservationCategoryKind) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *ConversationObservationCategoryKind) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type ConversationObservationPhases struct {
	ActiveUs  *uint64 `json:"active_us"`
	WaitingUs *uint64 `json:"waiting_us"`
	OutputUs  *uint64 `json:"output_us"`
	ToolsUs   *uint64 `json:"tools_us"`
	OtherUs   *uint64 `json:"other_us"`
}

func DefaultConversationObservationPhases() ConversationObservationPhases {
	return ConversationObservationPhases{
		ActiveUs:  nil,
		WaitingUs: nil,
		OutputUs:  nil,
		ToolsUs:   nil,
		OtherUs:   nil,
	}
}
func (value *ConversationObservationPhases) UnmarshalJSON(data []byte) error {
	type plain ConversationObservationPhases
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{}, false); err != nil {
		return err
	}
	*value = ConversationObservationPhases(decoded)
	return nil
}
func (value ConversationObservationPhases) MarshalJSON() ([]byte, error) {
	type plain ConversationObservationPhases
	encoded := plain(value)
	return json.Marshal(encoded)
}
