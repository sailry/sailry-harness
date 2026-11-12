// Hand-maintained v1 protocol declarations; keep them consistent with the fixtures and tolerance tests.

package protocol

import (
	"encoding/json"
)

type PluginStorageDeclaration struct {
	Collections []PluginStorageCollection `json:"collections"`
}

func DefaultPluginStorageDeclaration() PluginStorageDeclaration {
	return PluginStorageDeclaration{
		Collections: []PluginStorageCollection{},
	}
}
func (value *PluginStorageDeclaration) UnmarshalJSON(data []byte) error {
	type plain PluginStorageDeclaration
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"collections"}, true); err != nil {
		return err
	}
	*value = PluginStorageDeclaration(decoded)
	return nil
}
func (value PluginStorageDeclaration) MarshalJSON() ([]byte, error) {
	type plain PluginStorageDeclaration
	encoded := plain(value)
	encoded.Collections = normalizeCollections(encoded.Collections)
	return json.Marshal(encoded)
}

type PluginStorageCollection struct {
	Scope  PluginStorageScope             `json:"scope"`
	Key    *string                        `json:"key,omitempty"`
	Prefix *string                        `json:"prefix,omitempty"`
	Schema json.RawMessage                `json:"schema"`
	Index  *PluginStorageDeclarationIndex `json:"index,omitempty"`
}

func DefaultPluginStorageCollection() PluginStorageCollection {
	return PluginStorageCollection{
		Scope:  PluginStorageScope(""),
		Key:    nil,
		Prefix: nil,
		Schema: json.RawMessage("null"),
		Index:  nil,
	}
}
func (value *PluginStorageCollection) UnmarshalJSON(data []byte) error {
	type plain PluginStorageCollection
	decoded := plain{}
	decoded.Key = nil
	decoded.Prefix = nil
	decoded.Index = nil
	if err := DecodeObject(data, &decoded, []string{"scope", "schema"}, true); err != nil {
		return err
	}
	*value = PluginStorageCollection(decoded)
	return nil
}
func (value PluginStorageCollection) MarshalJSON() ([]byte, error) {
	type plain PluginStorageCollection
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginStorageDeclarationIndex struct {
	Semantic bool `json:"semantic"`
}

func DefaultPluginStorageDeclarationIndex() PluginStorageDeclarationIndex {
	return PluginStorageDeclarationIndex{
		Semantic: false,
	}
}
func (value *PluginStorageDeclarationIndex) UnmarshalJSON(data []byte) error {
	type plain PluginStorageDeclarationIndex
	decoded := plain{}
	decoded.Semantic = false
	if err := DecodeObject(data, &decoded, []string{}, true); err != nil {
		return err
	}
	*value = PluginStorageDeclarationIndex(decoded)
	return nil
}
func (value PluginStorageDeclarationIndex) MarshalJSON() ([]byte, error) {
	type plain PluginStorageDeclarationIndex
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginStorageScope string

const (
	PluginStorageScopeNode         PluginStorageScope = "node"
	PluginStorageScopeConversation PluginStorageScope = "conversation"
)

func (value PluginStorageScope) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *PluginStorageScope) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch PluginStorageScope(decoded) {
	case PluginStorageScopeNode, PluginStorageScopeConversation:
		*value = PluginStorageScope(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value PluginStorageScope) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *PluginStorageScope) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type PluginStorageIndex struct {
	Fields [2]string            `json:"fields"`
	Tags   []string             `json:"tags"`
	Order  int64                `json:"order"`
	Source *PluginStorageSource `json:"source,omitempty"`
}

func DefaultPluginStorageIndex() PluginStorageIndex {
	return PluginStorageIndex{
		Fields: [2]string{},
		Tags:   []string{},
		Order:  0,
		Source: nil,
	}
}
func (value *PluginStorageIndex) UnmarshalJSON(data []byte) error {
	type plain PluginStorageIndex
	decoded := plain{}
	decoded.Source = nil
	if err := DecodeObject(data, &decoded, []string{"fields", "tags", "order"}, true); err != nil {
		return err
	}
	*value = PluginStorageIndex(decoded)
	return nil
}
func (value PluginStorageIndex) MarshalJSON() ([]byte, error) {
	type plain PluginStorageIndex
	encoded := plain(value)
	encoded.Fields = normalizeCollections(encoded.Fields)
	encoded.Tags = normalizeCollections(encoded.Tags)
	return json.Marshal(encoded)
}

type PluginStorageSource struct {
	Key     string `json:"key"`
	Pointer string `json:"pointer"`
}

func DefaultPluginStorageSource() PluginStorageSource {
	return PluginStorageSource{
		Key:     "",
		Pointer: "",
	}
}
func (value *PluginStorageSource) UnmarshalJSON(data []byte) error {
	type plain PluginStorageSource
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"key", "pointer"}, true); err != nil {
		return err
	}
	*value = PluginStorageSource(decoded)
	return nil
}
func (value PluginStorageSource) MarshalJSON() ([]byte, error) {
	type plain PluginStorageSource
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginStorageSearch struct {
	Terms   []string      `json:"terms"`
	Query   *string       `json:"query,omitempty"`
	Mode    KnowledgeMode `json:"mode"`
	Weights [2]uint16     `json:"weights"`
	All     []string      `json:"all"`
	Any     []string      `json:"any"`
	Offset  uint16        `json:"offset"`
	Limit   uint16        `json:"limit"`
}

func DefaultPluginStorageSearch() PluginStorageSearch {
	return PluginStorageSearch{
		Terms:   []string{},
		Query:   nil,
		Mode:    KnowledgeModeHybrid,
		Weights: [2]uint16{},
		All:     []string{},
		Any:     []string{},
		Offset:  0,
		Limit:   0,
	}
}
func (value *PluginStorageSearch) UnmarshalJSON(data []byte) error {
	type plain PluginStorageSearch
	decoded := plain{}
	decoded.Terms = []string{}
	decoded.Query = nil
	decoded.Mode = KnowledgeModeKeyword
	decoded.Weights = [2]uint16{1, 1}
	decoded.All = []string{}
	decoded.Any = []string{}
	decoded.Offset = 0
	decoded.Limit = 100
	if err := DecodeObject(data, &decoded, []string{}, true); err != nil {
		return err
	}
	*value = PluginStorageSearch(decoded)
	return nil
}
func (value PluginStorageSearch) MarshalJSON() ([]byte, error) {
	type plain PluginStorageSearch
	encoded := plain(value)
	encoded.Terms = normalizeCollections(encoded.Terms)
	encoded.Weights = normalizeCollections(encoded.Weights)
	encoded.All = normalizeCollections(encoded.All)
	encoded.Any = normalizeCollections(encoded.Any)
	return json.Marshal(encoded)
}

type PluginStorageSearchPage struct {
	Entries  []PluginStorageEntry `json:"entries"`
	Next     *uint16              `json:"next"`
	NowMs    uint64               `json:"now_ms"`
	Mode     KnowledgeMode        `json:"mode"`
	Indexing bool                 `json:"indexing"`
}

func DefaultPluginStorageSearchPage() PluginStorageSearchPage {
	return PluginStorageSearchPage{
		Entries:  []PluginStorageEntry{},
		Next:     nil,
		NowMs:    0,
		Mode:     KnowledgeModeHybrid,
		Indexing: false,
	}
}
func (value *PluginStorageSearchPage) UnmarshalJSON(data []byte) error {
	type plain PluginStorageSearchPage
	decoded := plain{}
	decoded.Mode = KnowledgeModeKeyword
	decoded.Indexing = false
	if err := DecodeObject(data, &decoded, []string{"entries", "now_ms"}, false); err != nil {
		return err
	}
	*value = PluginStorageSearchPage(decoded)
	return nil
}
func (value PluginStorageSearchPage) MarshalJSON() ([]byte, error) {
	type plain PluginStorageSearchPage
	encoded := plain(value)
	encoded.Entries = normalizeCollections(encoded.Entries)
	return json.Marshal(encoded)
}

type PluginStorageEntry struct {
	Key      string          `json:"key"`
	Revision uint64          `json:"revision"`
	Value    json.RawMessage `json:"value"`
	Present  bool            `json:"present"`
}

func DefaultPluginStorageEntry() PluginStorageEntry {
	return PluginStorageEntry{
		Key:      "",
		Revision: 0,
		Value:    json.RawMessage("null"),
		Present:  false,
	}
}
func (value *PluginStorageEntry) UnmarshalJSON(data []byte) error {
	type plain PluginStorageEntry
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"key", "revision", "value", "present"}, false); err != nil {
		return err
	}
	*value = PluginStorageEntry(decoded)
	return nil
}
func (value PluginStorageEntry) MarshalJSON() ([]byte, error) {
	type plain PluginStorageEntry
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginStorageConversationEntry struct {
	Key      string          `json:"key"`
	Revision uint64          `json:"revision"`
	Value    json.RawMessage `json:"value"`
	Present  bool            `json:"present"`
	Restored bool            `json:"restored"`
}

func DefaultPluginStorageConversationEntry() PluginStorageConversationEntry {
	return PluginStorageConversationEntry{
		Key:      "",
		Revision: 0,
		Value:    json.RawMessage("null"),
		Present:  false,
		Restored: false,
	}
}
func (value *PluginStorageConversationEntry) UnmarshalJSON(data []byte) error {
	type plain PluginStorageConversationEntry
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"key", "revision", "value", "present", "restored"}, true); err != nil {
		return err
	}
	*value = PluginStorageConversationEntry(decoded)
	return nil
}
func (value PluginStorageConversationEntry) MarshalJSON() ([]byte, error) {
	type plain PluginStorageConversationEntry
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginStoragePage struct {
	Keys  []string `json:"keys"`
	After *string  `json:"after"`
}

func DefaultPluginStoragePage() PluginStoragePage {
	return PluginStoragePage{
		Keys:  []string{},
		After: nil,
	}
}
func (value *PluginStoragePage) UnmarshalJSON(data []byte) error {
	type plain PluginStoragePage
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"keys"}, false); err != nil {
		return err
	}
	*value = PluginStoragePage(decoded)
	return nil
}
func (value PluginStoragePage) MarshalJSON() ([]byte, error) {
	type plain PluginStoragePage
	encoded := plain(value)
	encoded.Keys = normalizeCollections(encoded.Keys)
	return json.Marshal(encoded)
}
