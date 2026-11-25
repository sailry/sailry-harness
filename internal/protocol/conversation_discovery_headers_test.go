package protocol

import (
	"bytes"
	"encoding/json"
	"reflect"
	"testing"
)

func TestDiscoveryDraftHeadersOptional(t *testing.T) {
	t.Parallel()
	for _, headers := range []map[string]string{nil, {}} {
		draft := DefaultConversationDiscoveryDraft()
		draft.Provider, draft.Api, draft.Endpoint = ProviderID{1}, ConversationModelApiResponses, "https://gateway.example/v1"
		draft.Headers = headers
		encoded, err := json.Marshal(draft)
		if err != nil || bytes.Contains(encoded, []byte(`"headers"`)) {
			t.Fatal("an empty discovery header override changed the legacy wire shape")
		}
		decoded := ConversationDiscoveryDraft{Headers: map[string]string{"X-Tenant-Id": "previous"}}
		if json.Unmarshal(encoded, &decoded) != nil || decoded.Headers != nil {
			t.Fatal("a legacy draft did not reset the optional headers to their default")
		}
		var legacy struct {
			Provider   ProviderID                    `json:"provider"`
			Api        ConversationModelApi          `json:"api"`
			Preset     *ConversationPresetsPresetRef `json:"preset,omitempty"`
			Endpoint   string                        `json:"endpoint"`
			Credential *CredentialRef                `json:"credential"`
			Secret     *Secret                       `json:"secret"`
		}
		strict := json.NewDecoder(bytes.NewReader(encoded))
		strict.DisallowUnknownFields()
		if strict.Decode(&legacy) != nil {
			t.Fatal("a strict legacy decoder rejected the unchanged discovery draft")
		}
		draft.Headers = map[string]string{"X-Tenant-Id": "private-discovery-fixture"}
		encoded, err = json.Marshal(draft)
		if err != nil {
			t.Fatal("encoding the discovery header override failed")
		}
		strict = json.NewDecoder(bytes.NewReader(encoded))
		strict.DisallowUnknownFields()
		if strict.Decode(&legacy) == nil {
			t.Fatal("the strict legacy boundary unexpectedly accepted an unsupported header override")
		}
	}
}

func TestDiscoveryDraftHeadersRoundTrip(t *testing.T) {
	t.Parallel()
	draft := DefaultConversationDiscoveryDraft()
	draft.Provider, draft.Api, draft.Endpoint = ProviderID{1}, ConversationModelApiResponses, "https://gateway.example/v1"
	draft.Headers = map[string]string{"X-Tenant-Id": "private-discovery-fixture", "User-Agent": "fixture-client"}
	command := Command{Kind: "discover_models", Data: ConversationDiscoverySource{Kind: "draft", Data: draft}}
	encoded, err := json.Marshal(command)
	if err != nil {
		t.Fatal("encoding a discovery command with headers failed")
	}
	var decoded Command
	if DecodeAs(PeerData, encoded, &decoded) != nil {
		t.Fatal("decoding a discovery command with headers failed")
	}
	source, ok := decoded.Data.(*ConversationDiscoverySource)
	if !ok || source.Kind != "draft" {
		t.Fatal("the discovery source changed during the wire round trip")
	}
	value, ok := source.Data.(*ConversationDiscoveryDraft)
	if !ok || !reflect.DeepEqual(value.Headers, draft.Headers) {
		t.Fatal("discovery headers changed during the wire round trip")
	}
	for _, invalid := range []string{`[]`, `{"X-Tenant-Id":true}`, `{"X-Tenant-Id":null}`} {
		encoded := []byte(`{"provider":"` + draft.Provider.String() + `","api":"` + string(draft.Api) + `","endpoint":"https://gateway.example/v1","headers":` + invalid + `}`)
		if json.Unmarshal(encoded, new(ConversationDiscoveryDraft)) == nil {
			t.Fatal("a discovery draft accepted headers outside the provider string-map contract")
		}
	}
}
