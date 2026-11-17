package protocol

import "encoding/json"

// ResourceSourceStatus reports one actual storage Node, including sources that
// could not answer. Availability is never inferred from an empty result set.
type ResourceSourceStatus struct {
	Node       NodeID  `json:"node"`
	Fault      *Fault  `json:"fault,omitempty"`
	NextOffset *uint32 `json:"next_offset,omitempty"`
}

// ResourceSource retains routing identity separately from the resources' Node.
// A catalog contains the entry itself and only its directly connected hosts.
type ResourceSource struct {
	Node      NodeID                `json:"node"`
	Route     RouteKey              `json:"route"`
	Name      string                `json:"name"`
	Platform  string                `json:"platform"`
	Projects  []Project             `json:"projects"`
	Worktrees []Worktree            `json:"worktrees"`
	Knowledge []KnowledgeCollection `json:"knowledge"`
	Fault     *Fault                `json:"fault,omitempty"`
	// Revision is the last complete source snapshot's cursor. Cached status is
	// historical whenever Fault reports that the source is unavailable.
	Revision       uint64  `json:"revision,omitempty"`
	LastSyncedAtMs *uint64 `json:"last_synced_at_ms,omitempty"`
}

func (value *ResourceSource) UnmarshalJSON(data []byte) error {
	type plain ResourceSource
	var decoded plain
	if err := DecodeObject(data, &decoded, []string{"node", "route", "projects", "worktrees", "knowledge"}, false); err != nil {
		return err
	}
	*value = ResourceSource(decoded)
	return nil
}

func (value ResourceSource) MarshalJSON() ([]byte, error) {
	type plain ResourceSource
	encoded := plain(value)
	encoded.Projects = normalizeCollections(encoded.Projects)
	encoded.Worktrees = normalizeCollections(encoded.Worktrees)
	encoded.Knowledge = normalizeCollections(encoded.Knowledge)
	return json.Marshal(encoded)
}

// ResourceCatalog is the initiating controller's unified resource directory.
// An unavailable source remains represented with its route and fault.
type ResourceCatalog struct {
	Entry   NodeID           `json:"entry"`
	Sources []ResourceSource `json:"sources"`
}

func (value *ResourceCatalog) UnmarshalJSON(data []byte) error {
	type plain ResourceCatalog
	var decoded plain
	if err := DecodeObject(data, &decoded, []string{"entry", "sources"}, false); err != nil {
		return err
	}
	*value = ResourceCatalog(decoded)
	return nil
}

func (value ResourceCatalog) MarshalJSON() ([]byte, error) {
	type plain ResourceCatalog
	encoded := plain(value)
	encoded.Sources = normalizeCollections(encoded.Sources)
	return json.Marshal(encoded)
}
