// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
)

type ConversationQuestionSpec struct {
	Prompt string                    `json:"prompt"`
	Input  ConversationQuestionInput `json:"input"`
}

func DefaultConversationQuestionSpec() ConversationQuestionSpec {
	return ConversationQuestionSpec{
		Prompt: "",
		Input:  ConversationQuestionInput{},
	}
}
func (value *ConversationQuestionSpec) UnmarshalJSON(data []byte) error { return Decode(data, value) }
func (value *ConversationQuestionSpec) DecodeJSON(decoder *Decoder) error {
	type plain ConversationQuestionSpec
	decoded := plain{}
	if err := decoder.Object(&decoded, []string{"prompt", "input"}, true); err != nil {
		return err
	}
	*value = ConversationQuestionSpec(decoded)
	return nil
}
func (value ConversationQuestionSpec) MarshalJSON() ([]byte, error) {
	type plain ConversationQuestionSpec
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ConversationQuestionInput struct {
	Kind string
	Data any
}
type ConversationQuestionInputTextPayload struct {
	Multiline bool   `json:"multiline"`
	MaxBytes  uint64 `json:"max_bytes"`
}

func DefaultConversationQuestionInputTextPayload() ConversationQuestionInputTextPayload {
	return ConversationQuestionInputTextPayload{
		Multiline: false,
		MaxBytes:  0,
	}
}
func (value *ConversationQuestionInputTextPayload) UnmarshalJSON(data []byte) error {
	type plain ConversationQuestionInputTextPayload
	decoded := plain{}
	decoded.Multiline = false
	decoded.MaxBytes = 64 * 1024
	if err := DecodeObject(data, &decoded, []string{}, true); err != nil {
		return err
	}
	*value = ConversationQuestionInputTextPayload(decoded)
	return nil
}
func (value ConversationQuestionInputTextPayload) MarshalJSON() ([]byte, error) {
	type plain ConversationQuestionInputTextPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ConversationQuestionInputChoicePayload struct {
	Options    []string `json:"options"`
	Multiple   bool     `json:"multiple"`
	AllowOther bool     `json:"allow_other"`
}

func DefaultConversationQuestionInputChoicePayload() ConversationQuestionInputChoicePayload {
	return ConversationQuestionInputChoicePayload{
		Options:    []string{},
		Multiple:   false,
		AllowOther: false,
	}
}
func (value *ConversationQuestionInputChoicePayload) UnmarshalJSON(data []byte) error {
	type plain ConversationQuestionInputChoicePayload
	decoded := plain{}
	decoded.AllowOther = false
	if err := DecodeObject(data, &decoded, []string{"options", "multiple"}, true); err != nil {
		return err
	}
	*value = ConversationQuestionInputChoicePayload(decoded)
	return nil
}
func (value ConversationQuestionInputChoicePayload) MarshalJSON() ([]byte, error) {
	type plain ConversationQuestionInputChoicePayload
	encoded := plain(value)
	encoded.Options = normalizeCollections(encoded.Options)
	return json.Marshal(encoded)
}

type ConversationQuestionInputFormPayload struct {
	Fields []ConversationQuestionFormField `json:"fields"`
}

func DefaultConversationQuestionInputFormPayload() ConversationQuestionInputFormPayload {
	return ConversationQuestionInputFormPayload{
		Fields: []ConversationQuestionFormField{},
	}
}
func (value *ConversationQuestionInputFormPayload) UnmarshalJSON(data []byte) error {
	type plain ConversationQuestionInputFormPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"fields"}, true); err != nil {
		return err
	}
	*value = ConversationQuestionInputFormPayload(decoded)
	return nil
}
func (value ConversationQuestionInputFormPayload) MarshalJSON() ([]byte, error) {
	type plain ConversationQuestionInputFormPayload
	encoded := plain(value)
	encoded.Fields = normalizeCollections(encoded.Fields)
	return json.Marshal(encoded)
}

type ConversationQuestionInputUrlPayload struct {
	Url           string `json:"url"`
	ElicitationID string `json:"elicitation_id"`
}

func DefaultConversationQuestionInputUrlPayload() ConversationQuestionInputUrlPayload {
	return ConversationQuestionInputUrlPayload{
		Url:           "",
		ElicitationID: "",
	}
}
func (value *ConversationQuestionInputUrlPayload) UnmarshalJSON(data []byte) error {
	type plain ConversationQuestionInputUrlPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"url", "elicitation_id"}, true); err != nil {
		return err
	}
	*value = ConversationQuestionInputUrlPayload(decoded)
	return nil
}
func (value ConversationQuestionInputUrlPayload) MarshalJSON() ([]byte, error) {
	type plain ConversationQuestionInputUrlPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var conversationQuestionInputVariants = variantFactories{"text": func() any { return new(ConversationQuestionInputTextPayload) },
	"choice": func() any { return new(ConversationQuestionInputChoicePayload) },
	"plan":   nil,
	"form":   func() any { return new(ConversationQuestionInputFormPayload) },
	"url":    func() any { return new(ConversationQuestionInputUrlPayload) }}

func (value ConversationQuestionInput) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "", value.Kind, value.Data, conversationQuestionInputVariants)
}
func (value *ConversationQuestionInput) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "", conversationQuestionInputVariants, true)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type ConversationQuestionAnswer struct {
	Kind string
	Data any
}
type ConversationQuestionAnswerChoicesPayload struct {
	Selected []uint64 `json:"selected"`
	Other    *string  `json:"other"`
}

func DefaultConversationQuestionAnswerChoicesPayload() ConversationQuestionAnswerChoicesPayload {
	return ConversationQuestionAnswerChoicesPayload{
		Selected: []uint64{},
		Other:    nil,
	}
}
func (value *ConversationQuestionAnswerChoicesPayload) UnmarshalJSON(data []byte) error {
	type plain ConversationQuestionAnswerChoicesPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"selected"}, true); err != nil {
		return err
	}
	*value = ConversationQuestionAnswerChoicesPayload(decoded)
	return nil
}
func (value ConversationQuestionAnswerChoicesPayload) MarshalJSON() ([]byte, error) {
	type plain ConversationQuestionAnswerChoicesPayload
	encoded := plain(value)
	encoded.Selected = normalizeCollections(encoded.Selected)
	return json.Marshal(encoded)
}

type ConversationQuestionAnswerPlanPayload struct {
	Turn TurnID `json:"turn"`
}

func DefaultConversationQuestionAnswerPlanPayload() ConversationQuestionAnswerPlanPayload {
	return ConversationQuestionAnswerPlanPayload{
		Turn: TurnID{},
	}
}
func (value *ConversationQuestionAnswerPlanPayload) UnmarshalJSON(data []byte) error {
	type plain ConversationQuestionAnswerPlanPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"turn"}, true); err != nil {
		return err
	}
	*value = ConversationQuestionAnswerPlanPayload(decoded)
	return nil
}
func (value ConversationQuestionAnswerPlanPayload) MarshalJSON() ([]byte, error) {
	type plain ConversationQuestionAnswerPlanPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var conversationQuestionAnswerVariants = variantFactories{"opened": nil,
	"form":    func() any { return new(map[string]json.RawMessage) },
	"text":    func() any { return new(string) },
	"choices": func() any { return new(ConversationQuestionAnswerChoicesPayload) },
	"plan":    func() any { return new(ConversationQuestionAnswerPlanPayload) }}

func (value ConversationQuestionAnswer) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "data", value.Kind, value.Data, conversationQuestionAnswerVariants)
}
func (value *ConversationQuestionAnswer) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "data", conversationQuestionAnswerVariants, true)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type ConversationQuestionResponse struct {
	Kind string
	Data any
}
type ConversationQuestionResponseStartCodingPayload struct {
	ExpectedRevision uint64            `json:"expected_revision"`
	Message          ConversationInput `json:"message"`
}

func DefaultConversationQuestionResponseStartCodingPayload() ConversationQuestionResponseStartCodingPayload {
	return ConversationQuestionResponseStartCodingPayload{
		ExpectedRevision: 0,
		Message:          DefaultConversationInput(),
	}
}
func (value *ConversationQuestionResponseStartCodingPayload) UnmarshalJSON(data []byte) error {
	type plain ConversationQuestionResponseStartCodingPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"expected_revision", "message"}, true); err != nil {
		return err
	}
	*value = ConversationQuestionResponseStartCodingPayload(decoded)
	return nil
}
func (value ConversationQuestionResponseStartCodingPayload) MarshalJSON() ([]byte, error) {
	type plain ConversationQuestionResponseStartCodingPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var conversationQuestionResponseVariants = variantFactories{"answer": func() any { return new(ConversationQuestionAnswer) },
	"decline":      nil,
	"cancel":       nil,
	"start_coding": func() any { return new(ConversationQuestionResponseStartCodingPayload) }}

func (value ConversationQuestionResponse) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "data", value.Kind, value.Data, conversationQuestionResponseVariants)
}
func (value *ConversationQuestionResponse) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "data", conversationQuestionResponseVariants, true)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type ConversationQuestionState struct {
	Kind string
	Data any
}

var conversationQuestionStateVariants = variantFactories{"pending": nil,
	"declined":    nil,
	"answered":    func() any { return new(ConversationQuestionAnswer) },
	"timed_out":   nil,
	"cancelled":   nil,
	"interrupted": nil}

func (value ConversationQuestionState) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "data", value.Kind, value.Data, conversationQuestionStateVariants)
}
func (value *ConversationQuestionState) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "data", conversationQuestionStateVariants, true)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type ConversationQuestion struct {
	ID      QuestionID                `json:"id"`
	Session SessionID                 `json:"session"`
	Turn    TurnID                    `json:"turn"`
	Entry   string                    `json:"entry"`
	Index   uint64                    `json:"index"`
	State   ConversationQuestionState `json:"state"`
}

func DefaultConversationQuestion() ConversationQuestion {
	return ConversationQuestion{
		ID:      QuestionID{},
		Session: SessionID{},
		Turn:    TurnID{},
		Entry:   "",
		Index:   0,
		State:   ConversationQuestionState{},
	}
}
func (value *ConversationQuestion) UnmarshalJSON(data []byte) error { return Decode(data, value) }
func (value *ConversationQuestion) DecodeJSON(decoder *Decoder) error {
	type plain ConversationQuestion
	decoded := plain{}
	if err := decoder.Object(&decoded, []string{"id", "session", "turn", "entry", "index", "state"}, false); err != nil {
		return err
	}
	*value = ConversationQuestion(decoded)
	return nil
}
func (value ConversationQuestion) MarshalJSON() ([]byte, error) {
	type plain ConversationQuestion
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ConversationQuestionAcceptedPlan struct {
	Question ConversationQuestion `json:"question"`
	Session  Session              `json:"session"`
	Turn     QueuedTurn           `json:"turn"`
}

func DefaultConversationQuestionAcceptedPlan() ConversationQuestionAcceptedPlan {
	return ConversationQuestionAcceptedPlan{
		Question: DefaultConversationQuestion(),
		Session:  DefaultSession(),
		Turn:     DefaultQueuedTurn(),
	}
}
func (value *ConversationQuestionAcceptedPlan) UnmarshalJSON(data []byte) error {
	type plain ConversationQuestionAcceptedPlan
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"question", "session", "turn"}, false); err != nil {
		return err
	}
	*value = ConversationQuestionAcceptedPlan(decoded)
	return nil
}
func (value ConversationQuestionAcceptedPlan) MarshalJSON() ([]byte, error) {
	type plain ConversationQuestionAcceptedPlan
	encoded := plain(value)
	return json.Marshal(encoded)
}
