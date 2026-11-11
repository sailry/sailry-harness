// Request layouts derive from rig-core 0.46.0 and the reviewed Sailry wire
// adapter. MIT and Apache-2.0; see LICENSE.rig.
package model

// Concrete request layouts follow the pinned Rig wire encoders. Raw declarations
// keep their own JSON order; SDKs continue to own HTTP and stream decoding.
import (
	"bytes"
	"encoding/json"
	"fmt"
	"slices"
	"sort"

	"charm.land/fantasy"
)

type sourceCache struct {
	Type string `json:"type"`
	TTL  string `json:"ttl,omitempty"`
}
type sourceAsset struct {
	Type      string `json:"type"`
	MediaType string `json:"media_type"`
	Data      string `json:"data"`
}
type sourceAnthropicBlock struct {
	Type         string          `json:"type"`
	Text         *string         `json:"text,omitempty"`
	Thinking     *string         `json:"thinking,omitempty"`
	Signature    *string         `json:"signature,omitempty"`
	Data         *string         `json:"data,omitempty"`
	ID           string          `json:"id,omitempty"`
	Name         string          `json:"name,omitempty"`
	Input        json.RawMessage `json:"input,omitempty"`
	ToolUseID    string          `json:"tool_use_id,omitempty"`
	Content      any             `json:"content,omitempty"`
	Source       *sourceAsset    `json:"source,omitempty"`
	IsError      *bool           `json:"is_error,omitempty"`
	CacheControl *sourceCache    `json:"cache_control,omitempty"`
}
type sourceAnthropicMessage struct {
	Role    string                 `json:"role"`
	Content []sourceAnthropicBlock `json:"content"`
}
type sourceAnthropicTool struct {
	Name                string          `json:"name"`
	Description         *string         `json:"description,omitempty"`
	InputSchema         json.RawMessage `json:"input_schema"`
	EagerInputStreaming *bool           `json:"eager_input_streaming,omitempty"`
	DeferLoading        *bool           `json:"defer_loading,omitempty"`
	CacheControl        *sourceCache    `json:"cache_control,omitempty"`
}
type sourceChatFunction struct {
	Name        string          `json:"name"`
	Description *string         `json:"description,omitempty"`
	Parameters  json.RawMessage `json:"parameters"`
	Strict      bool            `json:"strict"`
}
type sourceChatTool struct {
	Type     string             `json:"type"`
	Function sourceChatFunction `json:"function"`
}
type sourceChatMessage struct {
	Role             string  `json:"role"`
	ToolCallID       string  `json:"tool_call_id,omitempty"`
	Content          any     `json:"content,omitempty"`
	ToolCalls        any     `json:"tool_calls,omitempty"`
	ReasoningContent *string `json:"reasoning_content,omitempty"`
}
type sourceResponsesTool struct {
	Type        string          `json:"type"`
	Name        string          `json:"name"`
	Parameters  json.RawMessage `json:"parameters"`
	Strict      bool            `json:"strict"`
	Description *string         `json:"description,omitempty"`
}
type sourceResponsePart struct {
	Type     string  `json:"type"`
	Text     *string `json:"text,omitempty"`
	Detail   string  `json:"detail,omitempty"`
	ImageURL string  `json:"image_url,omitempty"`
	Filename string  `json:"filename,omitempty"`
	FileData string  `json:"file_data,omitempty"`
}
type sourceResponseItem struct {
	Type             string               `json:"type"`
	ID               string               `json:"id,omitempty"`
	Role             string               `json:"role,omitempty"`
	Content          []sourceResponsePart `json:"content,omitempty"`
	CallID           string               `json:"call_id,omitempty"`
	Name             string               `json:"name,omitempty"`
	Arguments        string               `json:"arguments,omitempty"`
	Output           any                  `json:"output,omitempty"`
	EncryptedContent *string              `json:"encrypted_content,omitempty"`
	Summary          json.RawMessage      `json:"summary,omitempty"`
}
type sourceInlineData struct {
	MimeType string `json:"mimeType"`
	Data     string `json:"data"`
}
type sourceGoogleCall struct {
	ID   string          `json:"id,omitempty"`
	Name string          `json:"name"`
	Args json.RawMessage `json:"args"`
}
type sourceGoogleResponse struct {
	Name     string `json:"name"`
	ID       string `json:"id,omitempty"`
	Response any    `json:"response"`
	Parts    any    `json:"parts,omitempty"`
}
type sourceGooglePart struct {
	Text             *string               `json:"text,omitempty"`
	Thought          *bool                 `json:"thought,omitempty"`
	ThoughtSignature string                `json:"thoughtSignature,omitempty"`
	InlineData       *sourceInlineData     `json:"inlineData,omitempty"`
	FunctionCall     *sourceGoogleCall     `json:"functionCall,omitempty"`
	FunctionResponse *sourceGoogleResponse `json:"functionResponse,omitempty"`
}
type sourceGoogleContent struct {
	Parts []sourceGooglePart `json:"parts"`
	Role  string             `json:"role"`
}
type sourceGoogleSystem struct {
	Parts []sourceGooglePart `json:"parts"`
}
type sourceGoogleDeclaration struct {
	Name                 string          `json:"name"`
	Description          *string         `json:"description,omitempty"`
	ParametersJSONSchema json.RawMessage `json:"parametersJsonSchema"`
}
type sourceGoogleTools struct {
	FunctionDeclarations []sourceGoogleDeclaration `json:"functionDeclarations"`
}

func decodeSource(value any, target any) error {
	raw, err := json.Marshal(value)
	if err != nil {
		return err
	}
	return json.Unmarshal(raw, target)
}

func sourceRequestBytes(config transportConfig, call fantasy.Call, body map[string]any) ([]byte, error) {
	var prefix, suffix []string
	switch config.Kind {
	case "anthropic":
		prefix = []string{"model", "messages", "max_tokens", "system", "tools"}
		suffix = []string{"stream", "tool_choice"}
	case "responses":
		prefix = []string{"model", "instructions", "temperature", "tools"}
		if slices.Contains(config.Order, "instructions") {
			prefix = []string{"model", "temperature", "tools"}
		}
		suffix = []string{"stream", "input", "max_output_tokens"}
	case "google":
		prefix = []string{"contents", "generationConfig", "tools", "systemInstruction"}
	default:
		prefix = []string{"model", "messages", "tools", "temperature"}
		suffix = []string{"max_tokens", "max_completion_tokens", "stream", "stream_options"}
	}
	order := append([]string(nil), prefix...)
	order = append(order, config.Order...)
	if params, ok := call.ProviderOptions[parametersKey].(*callParameters); ok {
		order = append(order, params.Order...)
	}
	var remaining []string
	for key := range body {
		if !slices.Contains(order, key) && !slices.Contains(suffix, key) {
			remaining = append(remaining, key)
		}
	}
	// The source model path adds these defaults before automatic reasoning fields.
	if config.Kind == "responses" && len(config.Order) == 0 {
		for _, key := range []string{"store", "prompt_cache_key", "include", "reasoning"} {
			if !slices.Contains(order, key) {
				order = append(order, key)
			}
		}
	}
	sort.Strings(remaining)
	order = append(order, remaining...)
	order = append(order, suffix...)
	return encodeSourceFields(config.Kind, body, order, call)
}

// These are the source wire's fixed struct fields and flattened request options.
// Every value is marshaled by encoding/json; this does not parse or repair JSON.
func encodeSourceFields(kind string, body map[string]any, order []string, call fantasy.Call) ([]byte, error) {
	var output bytes.Buffer
	output.WriteByte('{')
	written := map[string]bool{}
	separator := ""
	for _, key := range order {
		value, exists := body[key]
		if !exists || written[key] {
			continue
		}
		written[key] = true
		prepared, err := sourceValue(kind, key, value, call)
		if err != nil {
			return nil, err
		}
		output.WriteString(separator)
		separator = ","
		output.Write(claudeJSON(key))
		output.WriteByte(':')
		output.Write(claudeJSON(prepared))
	}
	output.WriteByte('}')
	return output.Bytes(), nil
}

func sourceValue(kind, key string, value any, call fantasy.Call) (any, error) {
	switch key {
	case "thinking":
		return sourcePolicyBytes(value, nil, []string{"type", "budget_tokens", "display"}, sourcePolicyRaw(call, key), call)
	case "reasoning":
		return sourcePolicyBytes(value, nil, []string{"effort", "summary"}, sourcePolicyRaw(call, key), call)
	case "generationConfig":
		var initial []string
		if call.MaxOutputTokens != nil {
			initial = append(initial, "maxOutputTokens")
		}
		if call.Temperature != nil {
			initial = append(initial, "temperature")
		}
		return sourcePolicyBytes(value, initial, []string{"maxOutputTokens", "temperature", "topP", "topK", "thinkingConfig"}, sourcePolicyRaw(call, key), call)
	case "systemInstruction":
		var result sourceGoogleSystem
		return &result, decodeSource(value, &result)
	case "contents":
		var result []sourceGoogleContent
		err := decodeSource(value, &result)
		return result, err
	case "input":
		if kind == "responses" {
			var result []sourceResponseItem
			err := decodeSource(value, &result)
			return result, err
		}
	case "messages":
		if kind == "anthropic" {
			var result []sourceAnthropicMessage
			err := decodeSource(value, &result)
			return result, err
		}
		var result []sourceChatMessage
		err := decodeSource(value, &result)
		return result, err
	case "system":
		if kind == "anthropic" {
			var result []sourceAnthropicBlock
			err := decodeSource(value, &result)
			return result, err
		}
	case "tools":
		return sourceTools(kind, value, call)
	}
	return value, nil
}

func sourceTools(kind string, value any, call fantasy.Call) (any, error) {
	switch kind {
	case "anthropic":
		var result []sourceAnthropicTool
		if err := decodeSource(value, &result); err != nil {
			return nil, err
		}
		for i := range result {
			var err error
			result[i].InputSchema, err = declarationSchema(call, result[i].Name, result[i].InputSchema, false)
			if err != nil {
				return nil, err
			}
		}
		return result, nil
	case "responses":
		var result []sourceResponsesTool
		if err := decodeSource(value, &result); err != nil {
			return nil, err
		}
		for i := range result {
			var err error
			result[i].Parameters, err = declarationSchema(call, result[i].Name, result[i].Parameters, false)
			if err != nil {
				return nil, err
			}
		}
		return result, nil
	case "google":
		tools := wireArray(value)
		if tools != nil {
			if len(tools) != 1 {
				return nil, fmt.Errorf("Gemini function declaration group is invalid")
			}
			value = tools[0]
		}
		var result sourceGoogleTools
		if err := decodeSource(value, &result); err != nil {
			return nil, err
		}
		for i := range result.FunctionDeclarations {
			var err error
			tool := &result.FunctionDeclarations[i]
			tool.ParametersJSONSchema, err = declarationSchema(call, tool.Name, tool.ParametersJSONSchema, false)
			if err != nil {
				return nil, err
			}
		}
		return result, nil
	default:
		var result []sourceChatTool
		if err := decodeSource(value, &result); err != nil {
			return nil, err
		}
		for i := range result {
			var err error
			tool := &result[i].Function
			tool.Parameters, err = declarationSchema(call, tool.Name, tool.Parameters, true)
			if err != nil {
				return nil, err
			}
		}
		return result, nil
	}
}

// Captured native option objects remain extensible. Keep the typed wire prefix,
// then the original policy member order and newly inserted source fields. The
// standard decoder reads only this concrete native-options object's members;
// opaque extension values remain raw JSON and never pass through a closed DTO.
func sourcePolicyRaw(call fantasy.Call, name string) json.RawMessage {
	if params, ok := call.ProviderOptions[parametersKey].(*callParameters); ok {
		return params.Raw[name]
	}
	return nil
}
func sourcePolicyBytes(value any, initial, known []string, original json.RawMessage, call fantasy.Call) (json.RawMessage, error) {
	current := wireObject(value)
	if current == nil {
		return json.Marshal(value)
	}
	body := make(map[string]any, len(current))
	for key, field := range current {
		body[key] = field
	}
	order := append([]string(nil), initial...)
	originals := map[string]json.RawMessage{}
	if len(original) > 0 {
		decoder := json.NewDecoder(bytes.NewReader(original))
		if token, err := decoder.Token(); err != nil || token != json.Delim('{') {
			return nil, fmt.Errorf("captured native options object is invalid")
		}
		for decoder.More() {
			name, err := decoder.Token()
			if err != nil {
				return nil, err
			}
			var raw json.RawMessage
			if err := decoder.Decode(&raw); err != nil {
				return nil, err
			}
			key := name.(string)
			originals[key] = raw
			order = append(order, key)
			if !slices.Contains(known, key) {
				if _, exists := body[key]; exists {
					body[key] = raw
				}
			}
		}
		if _, err := decoder.Token(); err != nil {
			return nil, err
		}
	}
	order = append(order, known...)
	var remaining []string
	for key := range body {
		if !slices.Contains(order, key) {
			remaining = append(remaining, key)
		}
	}
	sort.Strings(remaining)
	order = append(order, remaining...)
	if thinking, exists := current["thinkingConfig"]; exists {
		raw, err := sourcePolicyBytes(thinking, nil, []string{"thinkingBudget", "thinkingLevel", "includeThoughts"}, originals["thinkingConfig"], call)
		if err != nil {
			return nil, err
		}
		body["thinkingConfig"] = raw
	}
	raw, err := encodeSourceFields("native-options", body, order, call)
	return json.RawMessage(raw), err
}
