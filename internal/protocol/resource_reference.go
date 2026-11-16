package protocol

// HostPath identifies a file or directory on its owning Node. A path string
// alone must never be interpreted on another machine.
type HostPath struct {
	Node NodeID `json:"node"`
	Path string `json:"path"`
}

func (value *HostPath) UnmarshalJSON(data []byte) error {
	type plain HostPath
	var decoded plain
	if err := DecodeObject(data, &decoded, []string{"node", "path"}, false); err != nil {
		return err
	}
	*value = HostPath(decoded)
	return nil
}

// KnowledgeRef identifies a collection without assuming a controller's local
// store. Names and source paths are display metadata, not resource identities.
type KnowledgeRef struct {
	Node NodeID      `json:"node"`
	ID   KnowledgeID `json:"id"`
}

func (value *KnowledgeRef) UnmarshalJSON(data []byte) error {
	type plain KnowledgeRef
	var decoded plain
	if err := DecodeObject(data, &decoded, []string{"node", "id"}, false); err != nil {
		return err
	}
	*value = KnowledgeRef(decoded)
	return nil
}

// ResourceScope is carried only by the trusted runtime's resource-call channel.
// The controller verifies its source before it supplies the captured package to
// the resource Node. Ordinary commands cannot nominate this scope.
type ResourceScope struct {
	Plugin *PluginInfo    `json:"plugin,omitempty"`
	Origin *PluginContext `json:"origin,omitempty"`
}

func (value *ResourceScope) UnmarshalJSON(data []byte) error {
	type plain ResourceScope
	var decoded plain
	if err := DecodeObject(data, &decoded, nil, false); err != nil {
		return err
	}
	*value = ResourceScope(decoded)
	return nil
}
