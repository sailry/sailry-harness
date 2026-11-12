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

var conversationConnectionOptionsVariants = variantFactories{"azure_open_ai": func() any { return new(ConversationConnectionOptionsAzureOpenAiPayload) },
	"hosted": func() any { return new(ConversationConnectionOptionsHostedPayload) }}

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
