// Hand-maintained v1 protocol declarations; keep them consistent with the fixtures and tolerance tests.

package protocol

import (
	"encoding/json"
)

type DecisionRequest struct {
	Model     *string                     `json:"model,omitempty"`
	State     string                      `json:"state"`
	Questions map[string]DecisionQuestion `json:"questions"`
}

func DefaultDecisionRequest() DecisionRequest {
	return DecisionRequest{
		Model:     nil,
		State:     "",
		Questions: map[string]DecisionQuestion{},
	}
}
func (value *DecisionRequest) UnmarshalJSON(data []byte) error {
	type plain DecisionRequest
	decoded := plain{}
	decoded.Model = nil
	if err := DecodeObject(data, &decoded, []string{"state", "questions"}, true); err != nil {
		return err
	}
	*value = DecisionRequest(decoded)
	return nil
}
func (value DecisionRequest) MarshalJSON() ([]byte, error) {
	type plain DecisionRequest
	encoded := plain(value)
	encoded.Questions = normalizeCollections(encoded.Questions)
	return json.Marshal(encoded)
}

type DecisionQuestion struct {
	Kind string
	Data any
}
type DecisionQuestionNoulPayload struct {
	Instructions string            `json:"instructions"`
	Criteria     map[string]string `json:"criteria,omitempty"`
}

func DefaultDecisionQuestionNoulPayload() DecisionQuestionNoulPayload {
	return DecisionQuestionNoulPayload{
		Instructions: "",
		Criteria:     map[string]string{},
	}
}
func (value *DecisionQuestionNoulPayload) UnmarshalJSON(data []byte) error {
	type plain DecisionQuestionNoulPayload
	decoded := plain{}
	decoded.Criteria = map[string]string{}
	if err := DecodeObject(data, &decoded, []string{"instructions"}, true); err != nil {
		return err
	}
	*value = DecisionQuestionNoulPayload(decoded)
	return nil
}
func (value DecisionQuestionNoulPayload) MarshalJSON() ([]byte, error) {
	type plain DecisionQuestionNoulPayload
	encoded := plain(value)
	encoded.Criteria = normalizeCollections(encoded.Criteria)
	return json.Marshal(encoded)
}

type DecisionQuestionChoicePayload struct {
	Instructions string             `json:"instructions"`
	Criteria     map[string]*string `json:"criteria"`
}

func DefaultDecisionQuestionChoicePayload() DecisionQuestionChoicePayload {
	return DecisionQuestionChoicePayload{
		Instructions: "",
		Criteria:     map[string]*string{},
	}
}
func (value *DecisionQuestionChoicePayload) UnmarshalJSON(data []byte) error {
	type plain DecisionQuestionChoicePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"instructions", "criteria"}, true); err != nil {
		return err
	}
	*value = DecisionQuestionChoicePayload(decoded)
	return nil
}
func (value DecisionQuestionChoicePayload) MarshalJSON() ([]byte, error) {
	type plain DecisionQuestionChoicePayload
	encoded := plain(value)
	encoded.Criteria = normalizeCollections(encoded.Criteria)
	return json.Marshal(encoded)
}

type DecisionQuestionScorePayload struct {
	Instructions string   `json:"instructions"`
	Criteria     []string `json:"criteria"`
}

func DefaultDecisionQuestionScorePayload() DecisionQuestionScorePayload {
	return DecisionQuestionScorePayload{
		Instructions: "",
		Criteria:     []string{},
	}
}
func (value *DecisionQuestionScorePayload) UnmarshalJSON(data []byte) error {
	type plain DecisionQuestionScorePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"instructions", "criteria"}, true); err != nil {
		return err
	}
	*value = DecisionQuestionScorePayload(decoded)
	return nil
}
func (value DecisionQuestionScorePayload) MarshalJSON() ([]byte, error) {
	type plain DecisionQuestionScorePayload
	encoded := plain(value)
	encoded.Criteria = normalizeCollections(encoded.Criteria)
	return json.Marshal(encoded)
}

var decisionQuestionVariants = variantFactories{"noul": func() any { return new(DecisionQuestionNoulPayload) },
	"choice": func() any { return new(DecisionQuestionChoicePayload) },
	"score":  func() any { return new(DecisionQuestionScorePayload) }}

func (value DecisionQuestion) MarshalJSON() ([]byte, error) {
	return MarshalUnion("type", "", value.Kind, value.Data, decisionQuestionVariants)
}
func (value *DecisionQuestion) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "type", "", decisionQuestionVariants, true)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type DecisionResponse struct {
	Answers map[string]DecisionAnswer `json:"answers"`
	Usage   DecisionUsage             `json:"usage"`
}

func DefaultDecisionResponse() DecisionResponse {
	return DecisionResponse{
		Answers: map[string]DecisionAnswer{},
		Usage:   DefaultDecisionUsage(),
	}
}
func (value *DecisionResponse) UnmarshalJSON(data []byte) error {
	type plain DecisionResponse
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"answers", "usage"}, false); err != nil {
		return err
	}
	*value = DecisionResponse(decoded)
	return nil
}
func (value DecisionResponse) MarshalJSON() ([]byte, error) {
	type plain DecisionResponse
	encoded := plain(value)
	encoded.Answers = normalizeCollections(encoded.Answers)
	return json.Marshal(encoded)
}

type DecisionAnswer struct {
	Kind string
	Data any
}
type DecisionAnswerNoulPayload struct {
	Noul float32 `json:"noul"`
}

func DefaultDecisionAnswerNoulPayload() DecisionAnswerNoulPayload {
	return DecisionAnswerNoulPayload{
		Noul: 0,
	}
}
func (value *DecisionAnswerNoulPayload) UnmarshalJSON(data []byte) error {
	type plain DecisionAnswerNoulPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"noul"}, false); err != nil {
		return err
	}
	*value = DecisionAnswerNoulPayload(decoded)
	return nil
}
func (value DecisionAnswerNoulPayload) MarshalJSON() ([]byte, error) {
	type plain DecisionAnswerNoulPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type DecisionAnswerChoicePayload struct {
	Choice        string             `json:"choice"`
	Probabilities map[string]float32 `json:"probabilities"`
	Confidence    float32            `json:"confidence"`
}

func DefaultDecisionAnswerChoicePayload() DecisionAnswerChoicePayload {
	return DecisionAnswerChoicePayload{
		Choice:        "",
		Probabilities: map[string]float32{},
		Confidence:    0,
	}
}
func (value *DecisionAnswerChoicePayload) UnmarshalJSON(data []byte) error {
	type plain DecisionAnswerChoicePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"choice", "probabilities", "confidence"}, false); err != nil {
		return err
	}
	*value = DecisionAnswerChoicePayload(decoded)
	return nil
}
func (value DecisionAnswerChoicePayload) MarshalJSON() ([]byte, error) {
	type plain DecisionAnswerChoicePayload
	encoded := plain(value)
	encoded.Probabilities = normalizeCollections(encoded.Probabilities)
	return json.Marshal(encoded)
}

type DecisionAnswerScorePayload struct {
	Score         float32   `json:"score"`
	Legend        []string  `json:"legend"`
	Probabilities []float32 `json:"probabilities"`
	Confidence    float32   `json:"confidence"`
}

func DefaultDecisionAnswerScorePayload() DecisionAnswerScorePayload {
	return DecisionAnswerScorePayload{
		Score:         0,
		Legend:        []string{},
		Probabilities: []float32{},
		Confidence:    0,
	}
}
func (value *DecisionAnswerScorePayload) UnmarshalJSON(data []byte) error {
	type plain DecisionAnswerScorePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"score", "legend", "probabilities", "confidence"}, false); err != nil {
		return err
	}
	*value = DecisionAnswerScorePayload(decoded)
	return nil
}
func (value DecisionAnswerScorePayload) MarshalJSON() ([]byte, error) {
	type plain DecisionAnswerScorePayload
	encoded := plain(value)
	encoded.Legend = normalizeCollections(encoded.Legend)
	encoded.Probabilities = normalizeCollections(encoded.Probabilities)
	return json.Marshal(encoded)
}

var decisionAnswerVariants = variantFactories{"noul": func() any { return new(DecisionAnswerNoulPayload) },
	"choice": func() any { return new(DecisionAnswerChoicePayload) },
	"score":  func() any { return new(DecisionAnswerScorePayload) }}

func (value DecisionAnswer) MarshalJSON() ([]byte, error) {
	return MarshalUnion("type", "", value.Kind, value.Data, decisionAnswerVariants)
}
func (value *DecisionAnswer) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "type", "", decisionAnswerVariants, false)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type DecisionUsage struct {
	InputTokens  uint64 `json:"input_tokens"`
	OutputTokens uint64 `json:"output_tokens"`
}

func DefaultDecisionUsage() DecisionUsage {
	return DecisionUsage{
		InputTokens:  0,
		OutputTokens: 0,
	}
}
func (value *DecisionUsage) UnmarshalJSON(data []byte) error {
	type plain DecisionUsage
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"input_tokens", "output_tokens"}, false); err != nil {
		return err
	}
	*value = DecisionUsage(decoded)
	return nil
}
func (value DecisionUsage) MarshalJSON() ([]byte, error) {
	type plain DecisionUsage
	encoded := plain(value)
	return json.Marshal(encoded)
}
