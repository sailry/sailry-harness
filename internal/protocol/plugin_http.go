// Hand-maintained v1 protocol declarations; keep them consistent with the fixtures and tolerance tests.

package protocol

import (
	"encoding/json"
)

type PluginHttpRequest struct {
	Method     string             `json:"method"`
	Url        string             `json:"url"`
	Headers    map[string]string  `json:"headers"`
	Body       *string            `json:"body"`
	Credential *string            `json:"credential"`
	Connector  *string            `json:"connector"`
	Connection *PluginConnectorID `json:"connection"`
	Idempotent bool               `json:"idempotent"`
	TimeoutMs  uint32             `json:"timeout_ms"`
}

func DefaultPluginHttpRequest() PluginHttpRequest {
	return PluginHttpRequest{
		Method:     "",
		Url:        "",
		Headers:    map[string]string{},
		Body:       nil,
		Credential: nil,
		Connector:  nil,
		Connection: nil,
		Idempotent: false,
		TimeoutMs:  0,
	}
}
func (value *PluginHttpRequest) UnmarshalJSON(data []byte) error {
	type plain PluginHttpRequest
	decoded := plain{}
	decoded.Headers = map[string]string{}
	decoded.Connector = nil
	decoded.Connection = nil
	decoded.Idempotent = false
	decoded.TimeoutMs = 30000
	if err := DecodeObject(data, &decoded, []string{"method", "url"}, true); err != nil {
		return err
	}
	*value = PluginHttpRequest(decoded)
	return nil
}
func (value PluginHttpRequest) MarshalJSON() ([]byte, error) {
	type plain PluginHttpRequest
	encoded := plain(value)
	encoded.Headers = normalizeCollections(encoded.Headers)
	return json.Marshal(encoded)
}

type PluginHttpResponse struct {
	Status  uint16              `json:"status"`
	Headers map[string][]string `json:"headers"`
	Body    string              `json:"body"`
}

func DefaultPluginHttpResponse() PluginHttpResponse {
	return PluginHttpResponse{
		Status:  0,
		Headers: map[string][]string{},
		Body:    "",
	}
}
func (value *PluginHttpResponse) UnmarshalJSON(data []byte) error {
	type plain PluginHttpResponse
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"status", "headers", "body"}, false); err != nil {
		return err
	}
	*value = PluginHttpResponse(decoded)
	return nil
}
func (value PluginHttpResponse) MarshalJSON() ([]byte, error) {
	type plain PluginHttpResponse
	encoded := plain(value)
	encoded.Headers = normalizeCollections(encoded.Headers)
	return json.Marshal(encoded)
}
