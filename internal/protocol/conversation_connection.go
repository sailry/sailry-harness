// Hand-maintained v1 protocol declarations; keep them consistent with the fixtures and tolerance tests.

package protocol

import (
	"encoding/json"
)

type ConversationConnectionOptions struct {
	Kind string
	Data any
}
type ConversationConnectionOptionsAzureOpenAiPayload struct {
	ApiVersion string `json:"api_version"`
}

func DefaultConversationConnectionOptionsAzureOpenAiPayload() ConversationConnectionOptionsAzureOpenAiPayload {
	return ConversationConnectionOptionsAzureOpenAiPayload{
		ApiVersion: "",
	}
}
func (value *ConversationConnectionOptionsAzureOpenAiPayload) UnmarshalJSON(data []byte) error {
	type plain ConversationConnectionOptionsAzureOpenAiPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"api_version"}, false); err != nil {
		return err
	}
	*value = ConversationConnectionOptionsAzureOpenAiPayload(decoded)
	return nil
}
func (value ConversationConnectionOptionsAzureOpenAiPayload) MarshalJSON() ([]byte, error) {
	type plain ConversationConnectionOptionsAzureOpenAiPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ConversationConnectionOptionsHostedPayload struct {
	Product ConversationConnectionProduct `json:"product"`
}

func DefaultConversationConnectionOptionsHostedPayload() ConversationConnectionOptionsHostedPayload {
	return ConversationConnectionOptionsHostedPayload{
		Product: ConversationConnectionProduct(""),
	}
}
func (value *ConversationConnectionOptionsHostedPayload) UnmarshalJSON(data []byte) error {
	type plain ConversationConnectionOptionsHostedPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"product"}, false); err != nil {
		return err
	}
	*value = ConversationConnectionOptionsHostedPayload(decoded)
	return nil
}
func (value ConversationConnectionOptionsHostedPayload) MarshalJSON() ([]byte, error) {
	type plain ConversationConnectionOptionsHostedPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ConversationConnectionOptionsBedrockPayload struct {
	Region string `json:"region"`
}

func DefaultConversationConnectionOptionsBedrockPayload() ConversationConnectionOptionsBedrockPayload {
	return ConversationConnectionOptionsBedrockPayload{
		Region: "",
	}
}
func (value *ConversationConnectionOptionsBedrockPayload) UnmarshalJSON(data []byte) error {
	type plain ConversationConnectionOptionsBedrockPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"region"}, false); err != nil {
		return err
	}
	*value = ConversationConnectionOptionsBedrockPayload(decoded)
	return nil
}
func (value ConversationConnectionOptionsBedrockPayload) MarshalJSON() ([]byte, error) {
	type plain ConversationConnectionOptionsBedrockPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ConversationConnectionOptionsVertexAiPayload struct {
	Project  string `json:"project"`
	Location string `json:"location"`
}

func DefaultConversationConnectionOptionsVertexAiPayload() ConversationConnectionOptionsVertexAiPayload {
	return ConversationConnectionOptionsVertexAiPayload{
		Project:  "",
		Location: "",
	}
}
func (value *ConversationConnectionOptionsVertexAiPayload) UnmarshalJSON(data []byte) error {
	type plain ConversationConnectionOptionsVertexAiPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"project", "location"}, false); err != nil {
		return err
	}
	*value = ConversationConnectionOptionsVertexAiPayload(decoded)
	return nil
}
func (value ConversationConnectionOptionsVertexAiPayload) MarshalJSON() ([]byte, error) {
	type plain ConversationConnectionOptionsVertexAiPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var conversationConnectionOptionsVariants = variantFactories{"azure_open_ai": func() any { return new(ConversationConnectionOptionsAzureOpenAiPayload) },
	"bedrock":   func() any { return new(ConversationConnectionOptionsBedrockPayload) },
	"hosted":    func() any { return new(ConversationConnectionOptionsHostedPayload) },
	"vertex_ai": func() any { return new(ConversationConnectionOptionsVertexAiPayload) }}

// Endpoint is the Bedrock runtime endpoint of the region.
func (value ConversationConnectionOptionsBedrockPayload) Endpoint() string {
	return "https://bedrock-runtime." + value.Region + ".amazonaws.com"
}

// Endpoint is the Vertex AI service endpoint of the location: the global
// endpoint, a multi-region's regional endpoint, or the region's own.
func (value ConversationConnectionOptionsVertexAiPayload) Endpoint() string {
	switch value.Location {
	case "global":
		return "https://aiplatform.googleapis.com"
	case "us", "eu":
		return "https://aiplatform." + value.Location + ".rep.googleapis.com"
	}
	return "https://" + value.Location + "-aiplatform.googleapis.com"
}

// Bedrock is the Bedrock payload of options of that kind, decoded or built
// as a value or pointer.
func (value *ConversationConnectionOptions) Bedrock() (ConversationConnectionOptionsBedrockPayload, bool) {
	if value == nil || value.Kind != "bedrock" {
		return ConversationConnectionOptionsBedrockPayload{}, false
	}
	return connectionPayload[ConversationConnectionOptionsBedrockPayload](value.Data)
}

// VertexAi is the Vertex AI payload of options of that kind.
func (value *ConversationConnectionOptions) VertexAi() (ConversationConnectionOptionsVertexAiPayload, bool) {
	if value == nil || value.Kind != "vertex_ai" {
		return ConversationConnectionOptionsVertexAiPayload{}, false
	}
	return connectionPayload[ConversationConnectionOptionsVertexAiPayload](value.Data)
}

func connectionPayload[T any](data any) (T, bool) {
	switch v := data.(type) {
	case T:
		return v, true
	case *T:
		if v != nil {
			return *v, true
		}
	}
	var zero T
	return zero, false
}

func (value ConversationConnectionOptions) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "", value.Kind, value.Data, conversationConnectionOptionsVariants)
}
func (value *ConversationConnectionOptions) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "", conversationConnectionOptionsVariants, false)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type ConversationConnectionProduct string

const (
	ConversationConnectionProductMetered ConversationConnectionProduct = "metered"
	ConversationConnectionProductCoding  ConversationConnectionProduct = "coding"
)

func (value ConversationConnectionProduct) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *ConversationConnectionProduct) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch ConversationConnectionProduct(decoded) {
	case ConversationConnectionProductMetered, ConversationConnectionProductCoding:
		*value = ConversationConnectionProduct(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value ConversationConnectionProduct) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *ConversationConnectionProduct) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}
