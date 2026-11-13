package protocol

import "encoding/json"

type CommandReadPluginMemoryPayload struct{}

func (value *CommandReadPluginMemoryPayload) UnmarshalJSON(data []byte) error {
	type plain CommandReadPluginMemoryPayload
	return DecodeObject(data, (*plain)(value), nil, true)
}

// PluginMemorySnapshot contains only live Node JavaScript instances. These
// retained-heap estimates exclude shared, native and other Go-owned storage.
type PluginMemorySnapshot struct {
	Samples      []PluginMemorySample `json:"samples"`
	CapturedAtMs uint64               `json:"captured_at_ms"`
}

func DefaultPluginMemorySnapshot() PluginMemorySnapshot {
	return PluginMemorySnapshot{Samples: []PluginMemorySample{}}
}

func (value *PluginMemorySnapshot) UnmarshalJSON(data []byte) error {
	type plain PluginMemorySnapshot
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"samples", "captured_at_ms"}, true); err != nil {
		return err
	}
	*value = PluginMemorySnapshot(decoded)
	return nil
}

func (value PluginMemorySnapshot) MarshalJSON() ([]byte, error) {
	type plain PluginMemorySnapshot
	encoded := plain(value)
	encoded.Samples = normalizeCollections(encoded.Samples)
	return json.Marshal(encoded)
}

// PluginMemorySample groups instances by their exact captured package and
// execution kind (tool, callback or model). A nil Bytes means that at least
// one instance has no complete safe sample yet. Busy preserves the last
// complete estimate and its timestamp while an instance executes.
type PluginMemorySample struct {
	Package     PluginReference `json:"package"`
	Kind        string          `json:"kind"`
	Bytes       *uint64         `json:"bytes"`
	SampledAtMs uint64          `json:"sampled_at_ms"`
	Instances   uint32          `json:"instances"`
	Busy        bool            `json:"busy"`
}

func (value *PluginMemorySample) UnmarshalJSON(data []byte) error {
	type plain PluginMemorySample
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"package", "kind", "sampled_at_ms", "instances", "busy"}, true); err != nil {
		return err
	}
	*value = PluginMemorySample(decoded)
	return nil
}
