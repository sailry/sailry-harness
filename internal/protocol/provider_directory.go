package protocol

import "encoding/json"

// ProviderDirectory is the caller's provider projection at one Node event cursor.
type ProviderDirectory struct {
	Node      NodeID                      `json:"node"`
	Cursor    uint64                      `json:"cursor"`
	Providers []ConversationProvider      `json:"providers"`
	Quotas    []ConversationQuotaSnapshot `json:"quotas"`
}

func (value *ProviderDirectory) UnmarshalJSON(data []byte) error {
	type plain ProviderDirectory
	var decoded plain
	if err := DecodeObject(data, &decoded, []string{"node", "cursor", "providers", "quotas"}, false); err != nil {
		return err
	}
	*value = ProviderDirectory(decoded)
	return nil
}

func (value ProviderDirectory) MarshalJSON() ([]byte, error) {
	type plain ProviderDirectory
	encoded := plain(value)
	encoded.Providers = normalizeCollections(encoded.Providers)
	encoded.Quotas = normalizeCollections(encoded.Quotas)
	return json.Marshal(encoded)
}
