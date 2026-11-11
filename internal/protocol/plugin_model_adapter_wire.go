// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
)

type PluginModelAdapterWireRequest struct {
	Version         string                          `json:"version"`
	Model           string                          `json:"model"`
	Messages        []PluginModelAdapterWireMessage `json:"messages"`
	System          *string                         `json:"system"`
	Tools           []PluginModelAdapterWireTool    `json:"tools"`
	ToolChoice      PluginModelAdapterWireChoice    `json:"tool_choice"`
	Reasoning       PluginModelAdapterWireReasoning `json:"reasoning"`
	Features        map[string]string               `json:"features"`
	MaxOutputTokens *uint64                         `json:"max_output_tokens"`
	Temperature     *float64                        `json:"temperature"`
	TopP            *float64                        `json:"top_p"`
	Stop            []string                        `json:"stop"`
	Stream          bool                            `json:"stream"`
	Session         string                          `json:"session"`
	Endpoint        string                          `json:"endpoint"`
	Options         map[string]json.RawMessage      `json:"options"`
}

func DefaultPluginModelAdapterWireRequest() PluginModelAdapterWireRequest {
	return PluginModelAdapterWireRequest{
		Version:         "",
		Model:           "",
		Messages:        []PluginModelAdapterWireMessage{},
		System:          nil,
		Tools:           []PluginModelAdapterWireTool{},
		ToolChoice:      PluginModelAdapterWireChoice{},
		Reasoning:       DefaultPluginModelAdapterWireReasoning(),
		Features:        map[string]string{},
		MaxOutputTokens: nil,
		Temperature:     nil,
		TopP:            nil,
		Stop:            []string{},
		Stream:          false,
		Session:         "",
		Endpoint:        "",
		Options:         map[string]json.RawMessage{},
	}
}
func (value *PluginModelAdapterWireRequest) UnmarshalJSON(data []byte) error {
	type plain PluginModelAdapterWireRequest
	decoded := plain{}
	decoded.Features = map[string]string{}
	decoded.Stop = []string{}
	decoded.Options = map[string]json.RawMessage{}
	if err := DecodeObject(data, &decoded, []string{"version", "model", "messages", "tools", "tool_choice", "reasoning", "stream", "session", "endpoint"}, true); err != nil {
		return err
	}
	*value = PluginModelAdapterWireRequest(decoded)
	return nil
}
func (value PluginModelAdapterWireRequest) MarshalJSON() ([]byte, error) {
	type plain PluginModelAdapterWireRequest
	encoded := plain(value)
	encoded.Messages = normalizeCollections(encoded.Messages)
	encoded.Tools = normalizeCollections(encoded.Tools)
	encoded.Features = normalizeCollections(encoded.Features)
	encoded.Stop = normalizeCollections(encoded.Stop)
	encoded.Options = normalizeCollections(encoded.Options)
	return json.Marshal(encoded)
}

type PluginModelAdapterWireMessage struct {
	Kind string
	Data any
}
type PluginModelAdapterWireMessageUserPayload struct {
	Parts []PluginModelAdapterWirePart `json:"parts"`
}

func DefaultPluginModelAdapterWireMessageUserPayload() PluginModelAdapterWireMessageUserPayload {
	return PluginModelAdapterWireMessageUserPayload{
		Parts: []PluginModelAdapterWirePart{},
	}
}
func (value *PluginModelAdapterWireMessageUserPayload) UnmarshalJSON(data []byte) error {
	type plain PluginModelAdapterWireMessageUserPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"parts"}, true); err != nil {
		return err
	}
	*value = PluginModelAdapterWireMessageUserPayload(decoded)
	return nil
}
func (value PluginModelAdapterWireMessageUserPayload) MarshalJSON() ([]byte, error) {
	type plain PluginModelAdapterWireMessageUserPayload
	encoded := plain(value)
	encoded.Parts = normalizeCollections(encoded.Parts)
	return json.Marshal(encoded)
}

type PluginModelAdapterWireMessageAssistantPayload struct {
	Parts []PluginModelAdapterWirePart `json:"parts"`
}

func DefaultPluginModelAdapterWireMessageAssistantPayload() PluginModelAdapterWireMessageAssistantPayload {
	return PluginModelAdapterWireMessageAssistantPayload{
		Parts: []PluginModelAdapterWirePart{},
	}
}
func (value *PluginModelAdapterWireMessageAssistantPayload) UnmarshalJSON(data []byte) error {
	type plain PluginModelAdapterWireMessageAssistantPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"parts"}, true); err != nil {
		return err
	}
	*value = PluginModelAdapterWireMessageAssistantPayload(decoded)
	return nil
}
func (value PluginModelAdapterWireMessageAssistantPayload) MarshalJSON() ([]byte, error) {
	type plain PluginModelAdapterWireMessageAssistantPayload
	encoded := plain(value)
	encoded.Parts = normalizeCollections(encoded.Parts)
	return json.Marshal(encoded)
}

type PluginModelAdapterWireMessageToolPayload struct {
	CallID  string                       `json:"call_id"`
	Name    string                       `json:"name"`
	Parts   []PluginModelAdapterWirePart `json:"parts"`
	IsError bool                         `json:"is_error"`
}

func DefaultPluginModelAdapterWireMessageToolPayload() PluginModelAdapterWireMessageToolPayload {
	return PluginModelAdapterWireMessageToolPayload{
		CallID:  "",
		Name:    "",
		Parts:   []PluginModelAdapterWirePart{},
		IsError: false,
	}
}
func (value *PluginModelAdapterWireMessageToolPayload) UnmarshalJSON(data []byte) error {
	type plain PluginModelAdapterWireMessageToolPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"call_id", "name", "parts", "is_error"}, true); err != nil {
		return err
	}
	*value = PluginModelAdapterWireMessageToolPayload(decoded)
	return nil
}
func (value PluginModelAdapterWireMessageToolPayload) MarshalJSON() ([]byte, error) {
	type plain PluginModelAdapterWireMessageToolPayload
	encoded := plain(value)
	encoded.Parts = normalizeCollections(encoded.Parts)
	return json.Marshal(encoded)
}

var pluginModelAdapterWireMessageVariants = variantFactories{"user": func() any { return new(PluginModelAdapterWireMessageUserPayload) },
	"assistant": func() any { return new(PluginModelAdapterWireMessageAssistantPayload) },
	"tool":      func() any { return new(PluginModelAdapterWireMessageToolPayload) }}

func (value PluginModelAdapterWireMessage) MarshalJSON() ([]byte, error) {
	return MarshalUnion("role", "", value.Kind, value.Data, pluginModelAdapterWireMessageVariants)
}
func (value *PluginModelAdapterWireMessage) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "role", "", pluginModelAdapterWireMessageVariants, true)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type PluginModelAdapterWirePart struct {
	Kind string
	Data any
}
type PluginModelAdapterWirePartTextPayload struct {
	Text string `json:"text"`
}

func DefaultPluginModelAdapterWirePartTextPayload() PluginModelAdapterWirePartTextPayload {
	return PluginModelAdapterWirePartTextPayload{
		Text: "",
	}
}
func (value *PluginModelAdapterWirePartTextPayload) UnmarshalJSON(data []byte) error {
	type plain PluginModelAdapterWirePartTextPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"text"}, true); err != nil {
		return err
	}
	*value = PluginModelAdapterWirePartTextPayload(decoded)
	return nil
}
func (value PluginModelAdapterWirePartTextPayload) MarshalJSON() ([]byte, error) {
	type plain PluginModelAdapterWirePartTextPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginModelAdapterWirePartImagePayload struct {
	MediaType string                      `json:"media_type"`
	Data      PluginModelAdapterWireImage `json:"data"`
}

func DefaultPluginModelAdapterWirePartImagePayload() PluginModelAdapterWirePartImagePayload {
	return PluginModelAdapterWirePartImagePayload{
		MediaType: "",
		Data:      PluginModelAdapterWireImage{},
	}
}
func (value *PluginModelAdapterWirePartImagePayload) UnmarshalJSON(data []byte) error {
	type plain PluginModelAdapterWirePartImagePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"media_type", "data"}, true); err != nil {
		return err
	}
	*value = PluginModelAdapterWirePartImagePayload(decoded)
	return nil
}
func (value PluginModelAdapterWirePartImagePayload) MarshalJSON() ([]byte, error) {
	type plain PluginModelAdapterWirePartImagePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginModelAdapterWirePartFilePayload struct {
	MediaType  string  `json:"media_type"`
	Name       *string `json:"name"`
	DataBase64 string  `json:"data_base64"`
}

func DefaultPluginModelAdapterWirePartFilePayload() PluginModelAdapterWirePartFilePayload {
	return PluginModelAdapterWirePartFilePayload{
		MediaType:  "",
		Name:       nil,
		DataBase64: "",
	}
}
func (value *PluginModelAdapterWirePartFilePayload) UnmarshalJSON(data []byte) error {
	type plain PluginModelAdapterWirePartFilePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"media_type", "data_base64"}, true); err != nil {
		return err
	}
	*value = PluginModelAdapterWirePartFilePayload(decoded)
	return nil
}
func (value PluginModelAdapterWirePartFilePayload) MarshalJSON() ([]byte, error) {
	type plain PluginModelAdapterWirePartFilePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginModelAdapterWirePartReasoningPayload struct {
	Text   *string `json:"text"`
	Replay *string `json:"replay"`
}

func DefaultPluginModelAdapterWirePartReasoningPayload() PluginModelAdapterWirePartReasoningPayload {
	return PluginModelAdapterWirePartReasoningPayload{
		Text:   nil,
		Replay: nil,
	}
}
func (value *PluginModelAdapterWirePartReasoningPayload) UnmarshalJSON(data []byte) error {
	type plain PluginModelAdapterWirePartReasoningPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{}, true); err != nil {
		return err
	}
	*value = PluginModelAdapterWirePartReasoningPayload(decoded)
	return nil
}
func (value PluginModelAdapterWirePartReasoningPayload) MarshalJSON() ([]byte, error) {
	type plain PluginModelAdapterWirePartReasoningPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginModelAdapterWirePartToolCallPayload struct {
	ID        string          `json:"id"`
	Name      string          `json:"name"`
	Arguments json.RawMessage `json:"arguments"`
}

func DefaultPluginModelAdapterWirePartToolCallPayload() PluginModelAdapterWirePartToolCallPayload {
	return PluginModelAdapterWirePartToolCallPayload{
		ID:        "",
		Name:      "",
		Arguments: json.RawMessage("null"),
	}
}
func (value *PluginModelAdapterWirePartToolCallPayload) UnmarshalJSON(data []byte) error {
	type plain PluginModelAdapterWirePartToolCallPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id", "name", "arguments"}, true); err != nil {
		return err
	}
	*value = PluginModelAdapterWirePartToolCallPayload(decoded)
	return nil
}
func (value PluginModelAdapterWirePartToolCallPayload) MarshalJSON() ([]byte, error) {
	type plain PluginModelAdapterWirePartToolCallPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginModelAdapterWirePartJsonPayload struct {
	Value json.RawMessage `json:"value"`
}

func DefaultPluginModelAdapterWirePartJsonPayload() PluginModelAdapterWirePartJsonPayload {
	return PluginModelAdapterWirePartJsonPayload{
		Value: json.RawMessage("null"),
	}
}
func (value *PluginModelAdapterWirePartJsonPayload) UnmarshalJSON(data []byte) error {
	type plain PluginModelAdapterWirePartJsonPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"value"}, true); err != nil {
		return err
	}
	*value = PluginModelAdapterWirePartJsonPayload(decoded)
	return nil
}
func (value PluginModelAdapterWirePartJsonPayload) MarshalJSON() ([]byte, error) {
	type plain PluginModelAdapterWirePartJsonPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var pluginModelAdapterWirePartVariants = variantFactories{"text": func() any { return new(PluginModelAdapterWirePartTextPayload) },
	"image":     func() any { return new(PluginModelAdapterWirePartImagePayload) },
	"file":      func() any { return new(PluginModelAdapterWirePartFilePayload) },
	"reasoning": func() any { return new(PluginModelAdapterWirePartReasoningPayload) },
	"tool_call": func() any { return new(PluginModelAdapterWirePartToolCallPayload) },
	"json":      func() any { return new(PluginModelAdapterWirePartJsonPayload) }}

func (value PluginModelAdapterWirePart) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "", value.Kind, value.Data, pluginModelAdapterWirePartVariants)
}
func (value *PluginModelAdapterWirePart) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "", pluginModelAdapterWirePartVariants, true)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type PluginModelAdapterWireImage struct {
	Kind string
	Data any
}
type PluginModelAdapterWireImageInlinePayload struct {
	DataBase64 string `json:"data_base64"`
}

func DefaultPluginModelAdapterWireImageInlinePayload() PluginModelAdapterWireImageInlinePayload {
	return PluginModelAdapterWireImageInlinePayload{
		DataBase64: "",
	}
}
func (value *PluginModelAdapterWireImageInlinePayload) UnmarshalJSON(data []byte) error {
	type plain PluginModelAdapterWireImageInlinePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"data_base64"}, true); err != nil {
		return err
	}
	*value = PluginModelAdapterWireImageInlinePayload(decoded)
	return nil
}
func (value PluginModelAdapterWireImageInlinePayload) MarshalJSON() ([]byte, error) {
	type plain PluginModelAdapterWireImageInlinePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginModelAdapterWireImageUrlPayload struct {
	Url string `json:"url"`
}

func DefaultPluginModelAdapterWireImageUrlPayload() PluginModelAdapterWireImageUrlPayload {
	return PluginModelAdapterWireImageUrlPayload{
		Url: "",
	}
}
func (value *PluginModelAdapterWireImageUrlPayload) UnmarshalJSON(data []byte) error {
	type plain PluginModelAdapterWireImageUrlPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"url"}, true); err != nil {
		return err
	}
	*value = PluginModelAdapterWireImageUrlPayload(decoded)
	return nil
}
func (value PluginModelAdapterWireImageUrlPayload) MarshalJSON() ([]byte, error) {
	type plain PluginModelAdapterWireImageUrlPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var pluginModelAdapterWireImageVariants = variantFactories{"inline": func() any { return new(PluginModelAdapterWireImageInlinePayload) },
	"url": func() any { return new(PluginModelAdapterWireImageUrlPayload) }}

func (value PluginModelAdapterWireImage) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "", value.Kind, value.Data, pluginModelAdapterWireImageVariants)
}
func (value *PluginModelAdapterWireImage) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "", pluginModelAdapterWireImageVariants, true)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type PluginModelAdapterWireTool struct {
	Name        string          `json:"name"`
	Description *string         `json:"description"`
	Parameters  json.RawMessage `json:"parameters"`
}

func DefaultPluginModelAdapterWireTool() PluginModelAdapterWireTool {
	return PluginModelAdapterWireTool{
		Name:        "",
		Description: nil,
		Parameters:  json.RawMessage("null"),
	}
}
func (value *PluginModelAdapterWireTool) UnmarshalJSON(data []byte) error {
	type plain PluginModelAdapterWireTool
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"name", "parameters"}, true); err != nil {
		return err
	}
	*value = PluginModelAdapterWireTool(decoded)
	return nil
}
func (value PluginModelAdapterWireTool) MarshalJSON() ([]byte, error) {
	type plain PluginModelAdapterWireTool
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginModelAdapterWireChoice struct {
	Kind string
	Data any
}
type PluginModelAdapterWireChoiceNamedPayload struct {
	Name string `json:"name"`
}

func DefaultPluginModelAdapterWireChoiceNamedPayload() PluginModelAdapterWireChoiceNamedPayload {
	return PluginModelAdapterWireChoiceNamedPayload{
		Name: "",
	}
}
func (value *PluginModelAdapterWireChoiceNamedPayload) UnmarshalJSON(data []byte) error {
	type plain PluginModelAdapterWireChoiceNamedPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"name"}, true); err != nil {
		return err
	}
	*value = PluginModelAdapterWireChoiceNamedPayload(decoded)
	return nil
}
func (value PluginModelAdapterWireChoiceNamedPayload) MarshalJSON() ([]byte, error) {
	type plain PluginModelAdapterWireChoiceNamedPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var pluginModelAdapterWireChoiceVariants = variantFactories{"auto": nil,
	"none":     nil,
	"required": nil,
	"named":    func() any { return new(PluginModelAdapterWireChoiceNamedPayload) }}

func (value PluginModelAdapterWireChoice) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "", value.Kind, value.Data, pluginModelAdapterWireChoiceVariants)
}
func (value *PluginModelAdapterWireChoice) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "", pluginModelAdapterWireChoiceVariants, true)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type PluginModelAdapterWireReasoning struct {
	Effort Effort `json:"effort"`
	Replay bool   `json:"replay"`
}

func DefaultPluginModelAdapterWireReasoning() PluginModelAdapterWireReasoning {
	return PluginModelAdapterWireReasoning{
		Effort: Effort{Kind: "default"},
		Replay: false,
	}
}
func (value *PluginModelAdapterWireReasoning) UnmarshalJSON(data []byte) error {
	type plain PluginModelAdapterWireReasoning
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"effort", "replay"}, true); err != nil {
		return err
	}
	*value = PluginModelAdapterWireReasoning(decoded)
	return nil
}
func (value PluginModelAdapterWireReasoning) MarshalJSON() ([]byte, error) {
	type plain PluginModelAdapterWireReasoning
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginModelAdapterWirePrepared struct {
	Method     PluginModelAuthMethod            `json:"method"`
	Url        string                           `json:"url"`
	Headers    map[string]string                `json:"headers"`
	Body       *string                          `json:"body"`
	Credential *PluginModelAdapterWireInjection `json:"credential"`
	Framing    *PluginModelAdapterFraming       `json:"framing"`
}

func DefaultPluginModelAdapterWirePrepared() PluginModelAdapterWirePrepared {
	return PluginModelAdapterWirePrepared{
		Method:     PluginModelAuthMethodPost,
		Url:        "",
		Headers:    map[string]string{},
		Body:       nil,
		Credential: nil,
		Framing:    nil,
	}
}
func (value *PluginModelAdapterWirePrepared) UnmarshalJSON(data []byte) error {
	type plain PluginModelAdapterWirePrepared
	decoded := plain{}
	decoded.Headers = map[string]string{}
	if err := DecodeObject(data, &decoded, []string{"method", "url"}, true); err != nil {
		return err
	}
	*value = PluginModelAdapterWirePrepared(decoded)
	return nil
}
func (value PluginModelAdapterWirePrepared) MarshalJSON() ([]byte, error) {
	type plain PluginModelAdapterWirePrepared
	encoded := plain(value)
	encoded.Headers = normalizeCollections(encoded.Headers)
	return json.Marshal(encoded)
}

type PluginModelAdapterWireInjection struct {
	Header   string `json:"header"`
	Template string `json:"template"`
}

func DefaultPluginModelAdapterWireInjection() PluginModelAdapterWireInjection {
	return PluginModelAdapterWireInjection{
		Header:   "",
		Template: "",
	}
}
func (value *PluginModelAdapterWireInjection) UnmarshalJSON(data []byte) error {
	type plain PluginModelAdapterWireInjection
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"header", "template"}, true); err != nil {
		return err
	}
	*value = PluginModelAdapterWireInjection(decoded)
	return nil
}
func (value PluginModelAdapterWireInjection) MarshalJSON() ([]byte, error) {
	type plain PluginModelAdapterWireInjection
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginModelAdapterWireDecode struct {
	State   json.RawMessage               `json:"state"`
	Events  []PluginModelAdapterWireEvent `json:"events"`
	Done    bool                          `json:"done"`
	Status  *uint16                       `json:"status"`
	Headers *map[string][]string          `json:"headers"`
}

func DefaultPluginModelAdapterWireDecode() PluginModelAdapterWireDecode {
	return PluginModelAdapterWireDecode{
		State:   json.RawMessage("null"),
		Events:  []PluginModelAdapterWireEvent{},
		Done:    false,
		Status:  nil,
		Headers: nil,
	}
}
func (value *PluginModelAdapterWireDecode) UnmarshalJSON(data []byte) error {
	type plain PluginModelAdapterWireDecode
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"state", "events", "done"}, true); err != nil {
		return err
	}
	*value = PluginModelAdapterWireDecode(decoded)
	return nil
}
func (value PluginModelAdapterWireDecode) MarshalJSON() ([]byte, error) {
	type plain PluginModelAdapterWireDecode
	encoded := plain(value)
	encoded.Events = normalizeCollections(encoded.Events)
	encoded.Headers = normalizeCollections(encoded.Headers)
	return json.Marshal(encoded)
}

type PluginModelAdapterWireEvent struct {
	Event *string `json:"event"`
	Data  string  `json:"data"`
}

func DefaultPluginModelAdapterWireEvent() PluginModelAdapterWireEvent {
	return PluginModelAdapterWireEvent{
		Event: nil,
		Data:  "",
	}
}
func (value *PluginModelAdapterWireEvent) UnmarshalJSON(data []byte) error {
	type plain PluginModelAdapterWireEvent
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"data"}, true); err != nil {
		return err
	}
	*value = PluginModelAdapterWireEvent(decoded)
	return nil
}
func (value PluginModelAdapterWireEvent) MarshalJSON() ([]byte, error) {
	type plain PluginModelAdapterWireEvent
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginModelAdapterWireDecoded struct {
	State  json.RawMessage               `json:"state"`
	Deltas []PluginModelAdapterWireDelta `json:"deltas"`
}

func DefaultPluginModelAdapterWireDecoded() PluginModelAdapterWireDecoded {
	return PluginModelAdapterWireDecoded{
		State:  json.RawMessage("null"),
		Deltas: []PluginModelAdapterWireDelta{},
	}
}
func (value *PluginModelAdapterWireDecoded) UnmarshalJSON(data []byte) error {
	type plain PluginModelAdapterWireDecoded
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"state", "deltas"}, true); err != nil {
		return err
	}
	*value = PluginModelAdapterWireDecoded(decoded)
	return nil
}
func (value PluginModelAdapterWireDecoded) MarshalJSON() ([]byte, error) {
	type plain PluginModelAdapterWireDecoded
	encoded := plain(value)
	encoded.Deltas = normalizeCollections(encoded.Deltas)
	return json.Marshal(encoded)
}

type PluginModelAdapterWireDelta struct {
	Kind string
	Data any
}
type PluginModelAdapterWireDeltaTextPayload struct {
	Text string `json:"text"`
}

func DefaultPluginModelAdapterWireDeltaTextPayload() PluginModelAdapterWireDeltaTextPayload {
	return PluginModelAdapterWireDeltaTextPayload{
		Text: "",
	}
}
func (value *PluginModelAdapterWireDeltaTextPayload) UnmarshalJSON(data []byte) error {
	type plain PluginModelAdapterWireDeltaTextPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"text"}, true); err != nil {
		return err
	}
	*value = PluginModelAdapterWireDeltaTextPayload(decoded)
	return nil
}
func (value PluginModelAdapterWireDeltaTextPayload) MarshalJSON() ([]byte, error) {
	type plain PluginModelAdapterWireDeltaTextPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginModelAdapterWireDeltaReasoningPayload struct {
	Text   *string `json:"text"`
	Replay *string `json:"replay"`
}

func DefaultPluginModelAdapterWireDeltaReasoningPayload() PluginModelAdapterWireDeltaReasoningPayload {
	return PluginModelAdapterWireDeltaReasoningPayload{
		Text:   nil,
		Replay: nil,
	}
}
func (value *PluginModelAdapterWireDeltaReasoningPayload) UnmarshalJSON(data []byte) error {
	type plain PluginModelAdapterWireDeltaReasoningPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{}, true); err != nil {
		return err
	}
	*value = PluginModelAdapterWireDeltaReasoningPayload(decoded)
	return nil
}
func (value PluginModelAdapterWireDeltaReasoningPayload) MarshalJSON() ([]byte, error) {
	type plain PluginModelAdapterWireDeltaReasoningPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginModelAdapterWireDeltaToolCallPayload struct {
	Index          uint64  `json:"index"`
	ID             *string `json:"id"`
	Name           *string `json:"name"`
	ArgumentsDelta *string `json:"arguments_delta"`
}

func DefaultPluginModelAdapterWireDeltaToolCallPayload() PluginModelAdapterWireDeltaToolCallPayload {
	return PluginModelAdapterWireDeltaToolCallPayload{
		Index:          0,
		ID:             nil,
		Name:           nil,
		ArgumentsDelta: nil,
	}
}
func (value *PluginModelAdapterWireDeltaToolCallPayload) UnmarshalJSON(data []byte) error {
	type plain PluginModelAdapterWireDeltaToolCallPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"index"}, true); err != nil {
		return err
	}
	*value = PluginModelAdapterWireDeltaToolCallPayload(decoded)
	return nil
}
func (value PluginModelAdapterWireDeltaToolCallPayload) MarshalJSON() ([]byte, error) {
	type plain PluginModelAdapterWireDeltaToolCallPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginModelAdapterWireDeltaUsagePayload struct {
	InputTokens       *uint64 `json:"input_tokens"`
	OutputTokens      *uint64 `json:"output_tokens"`
	CachedInputTokens *uint64 `json:"cached_input_tokens"`
	ReasoningTokens   *uint64 `json:"reasoning_tokens"`
}

func DefaultPluginModelAdapterWireDeltaUsagePayload() PluginModelAdapterWireDeltaUsagePayload {
	return PluginModelAdapterWireDeltaUsagePayload{
		InputTokens:       nil,
		OutputTokens:      nil,
		CachedInputTokens: nil,
		ReasoningTokens:   nil,
	}
}
func (value *PluginModelAdapterWireDeltaUsagePayload) UnmarshalJSON(data []byte) error {
	type plain PluginModelAdapterWireDeltaUsagePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{}, true); err != nil {
		return err
	}
	*value = PluginModelAdapterWireDeltaUsagePayload(decoded)
	return nil
}
func (value PluginModelAdapterWireDeltaUsagePayload) MarshalJSON() ([]byte, error) {
	type plain PluginModelAdapterWireDeltaUsagePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginModelAdapterWireDeltaFinishPayload struct {
	Reason PluginModelAdapterWireFinish `json:"reason"`
}

func DefaultPluginModelAdapterWireDeltaFinishPayload() PluginModelAdapterWireDeltaFinishPayload {
	return PluginModelAdapterWireDeltaFinishPayload{
		Reason: PluginModelAdapterWireFinish(""),
	}
}
func (value *PluginModelAdapterWireDeltaFinishPayload) UnmarshalJSON(data []byte) error {
	type plain PluginModelAdapterWireDeltaFinishPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"reason"}, true); err != nil {
		return err
	}
	*value = PluginModelAdapterWireDeltaFinishPayload(decoded)
	return nil
}
func (value PluginModelAdapterWireDeltaFinishPayload) MarshalJSON() ([]byte, error) {
	type plain PluginModelAdapterWireDeltaFinishPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginModelAdapterWireDeltaErrorPayload struct {
	Class            PluginModelAdapterWireClass `json:"class"`
	Message          string                      `json:"message"`
	RetryAfterMs     *uint64                     `json:"retry_after_ms"`
	Code             *string                     `json:"code"`
	ErrorType        *string                     `json:"error_type"`
	EventType        *string                     `json:"event_type"`
	IncompleteReason *string                     `json:"incomplete_reason"`
}

func DefaultPluginModelAdapterWireDeltaErrorPayload() PluginModelAdapterWireDeltaErrorPayload {
	return PluginModelAdapterWireDeltaErrorPayload{
		Class:            PluginModelAdapterWireClass(""),
		Message:          "",
		RetryAfterMs:     nil,
		Code:             nil,
		ErrorType:        nil,
		EventType:        nil,
		IncompleteReason: nil,
	}
}
func (value *PluginModelAdapterWireDeltaErrorPayload) UnmarshalJSON(data []byte) error {
	type plain PluginModelAdapterWireDeltaErrorPayload
	decoded := plain{}
	decoded.Code = nil
	decoded.ErrorType = nil
	decoded.EventType = nil
	decoded.IncompleteReason = nil
	if err := DecodeObject(data, &decoded, []string{"class", "message"}, true); err != nil {
		return err
	}
	*value = PluginModelAdapterWireDeltaErrorPayload(decoded)
	return nil
}
func (value PluginModelAdapterWireDeltaErrorPayload) MarshalJSON() ([]byte, error) {
	type plain PluginModelAdapterWireDeltaErrorPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var pluginModelAdapterWireDeltaVariants = variantFactories{"text": func() any { return new(PluginModelAdapterWireDeltaTextPayload) },
	"reasoning": func() any { return new(PluginModelAdapterWireDeltaReasoningPayload) },
	"tool_call": func() any { return new(PluginModelAdapterWireDeltaToolCallPayload) },
	"usage":     func() any { return new(PluginModelAdapterWireDeltaUsagePayload) },
	"finish":    func() any { return new(PluginModelAdapterWireDeltaFinishPayload) },
	"error":     func() any { return new(PluginModelAdapterWireDeltaErrorPayload) }}

func (value PluginModelAdapterWireDelta) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "", value.Kind, value.Data, pluginModelAdapterWireDeltaVariants)
}
func (value *PluginModelAdapterWireDelta) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "", pluginModelAdapterWireDeltaVariants, true)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type PluginModelAdapterWireFinish string

const (
	PluginModelAdapterWireFinishStop          PluginModelAdapterWireFinish = "stop"
	PluginModelAdapterWireFinishLength        PluginModelAdapterWireFinish = "length"
	PluginModelAdapterWireFinishToolCalls     PluginModelAdapterWireFinish = "tool_calls"
	PluginModelAdapterWireFinishContentFilter PluginModelAdapterWireFinish = "content_filter"
	PluginModelAdapterWireFinishOther         PluginModelAdapterWireFinish = "other"
)

func (value PluginModelAdapterWireFinish) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *PluginModelAdapterWireFinish) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch PluginModelAdapterWireFinish(decoded) {
	case PluginModelAdapterWireFinishStop, PluginModelAdapterWireFinishLength, PluginModelAdapterWireFinishToolCalls, PluginModelAdapterWireFinishContentFilter, PluginModelAdapterWireFinishOther:
		*value = PluginModelAdapterWireFinish(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value PluginModelAdapterWireFinish) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *PluginModelAdapterWireFinish) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type PluginModelAdapterWireClass string

const (
	PluginModelAdapterWireClassAuthentication  PluginModelAdapterWireClass = "authentication"
	PluginModelAdapterWireClassPermission      PluginModelAdapterWireClass = "permission"
	PluginModelAdapterWireClassRateLimited     PluginModelAdapterWireClass = "rate_limited"
	PluginModelAdapterWireClassQuota           PluginModelAdapterWireClass = "quota"
	PluginModelAdapterWireClassContextOverflow PluginModelAdapterWireClass = "context_overflow"
	PluginModelAdapterWireClassInvalidRequest  PluginModelAdapterWireClass = "invalid_request"
	PluginModelAdapterWireClassUnavailable     PluginModelAdapterWireClass = "unavailable"
	PluginModelAdapterWireClassUnsupported     PluginModelAdapterWireClass = "unsupported"
	PluginModelAdapterWireClassUnknown         PluginModelAdapterWireClass = "unknown"
)

func (value PluginModelAdapterWireClass) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *PluginModelAdapterWireClass) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch PluginModelAdapterWireClass(decoded) {
	case PluginModelAdapterWireClassAuthentication, PluginModelAdapterWireClassPermission, PluginModelAdapterWireClassRateLimited, PluginModelAdapterWireClassQuota, PluginModelAdapterWireClassContextOverflow, PluginModelAdapterWireClassInvalidRequest, PluginModelAdapterWireClassUnavailable, PluginModelAdapterWireClassUnsupported, PluginModelAdapterWireClassUnknown:
		*value = PluginModelAdapterWireClass(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value PluginModelAdapterWireClass) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *PluginModelAdapterWireClass) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type PluginModelAdapterWireDiagnostics struct {
	Code             *string `json:"code,omitempty"`
	ErrorType        *string `json:"error_type,omitempty"`
	EventType        *string `json:"event_type,omitempty"`
	IncompleteReason *string `json:"incomplete_reason,omitempty"`
}

func DefaultPluginModelAdapterWireDiagnostics() PluginModelAdapterWireDiagnostics {
	return PluginModelAdapterWireDiagnostics{
		Code:             nil,
		ErrorType:        nil,
		EventType:        nil,
		IncompleteReason: nil,
	}
}
func (value *PluginModelAdapterWireDiagnostics) UnmarshalJSON(data []byte) error {
	type plain PluginModelAdapterWireDiagnostics
	decoded := plain{}
	decoded.Code = nil
	decoded.ErrorType = nil
	decoded.EventType = nil
	decoded.IncompleteReason = nil
	if err := DecodeObject(data, &decoded, []string{}, false); err != nil {
		return err
	}
	*value = PluginModelAdapterWireDiagnostics(decoded)
	return nil
}
func (value PluginModelAdapterWireDiagnostics) MarshalJSON() ([]byte, error) {
	type plain PluginModelAdapterWireDiagnostics
	encoded := plain(value)
	return json.Marshal(encoded)
}
