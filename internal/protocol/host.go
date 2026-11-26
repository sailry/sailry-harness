// Hand-maintained v1 protocol declarations; keep them consistent with the fixtures and tolerance tests.

package protocol

import (
	"encoding/json"
)

type HostTool struct {
	Name      string  `json:"name"`
	Available bool    `json:"available"`
	Path      *string `json:"path,omitempty"`
	Version   *string `json:"version,omitempty"`
}

func DefaultHostTool() HostTool {
	return HostTool{}
}
func (value *HostTool) UnmarshalJSON(data []byte) error { return Decode(data, value) }
func (value *HostTool) DecodeJSON(decoder *Decoder) error {
	type plain HostTool
	decoded := plain{}
	if err := decoder.Object(&decoded, []string{"name", "available"}, false); err != nil {
		return err
	}
	*value = HostTool(decoded)
	return nil
}
func (value HostTool) MarshalJSON() ([]byte, error) {
	type plain HostTool
	encoded := plain(value)
	return json.Marshal(encoded)
}

type HostInfo struct {
	Node           NodeID       `json:"node"`
	SampledAtMs    uint64       `json:"sampled_at_ms"`
	Name           *string      `json:"name"`
	Os             *string      `json:"os"`
	OsVersion      *string      `json:"os_version"`
	Architecture   *string      `json:"architecture"`
	LogicalCpus    *uint32      `json:"logical_cpus"`
	Memory         *MemoryUsage `json:"memory"`
	HostUptimeSecs uint64       `json:"host_uptime_secs"`
	NodeUptimeSecs uint64       `json:"node_uptime_secs"`
	Timezone       *string      `json:"timezone,omitempty"`
	Tools          []HostTool   `json:"tools,omitempty"`
}

func DefaultHostInfo() HostInfo {
	return HostInfo{
		Node:           NodeID{},
		SampledAtMs:    0,
		Name:           nil,
		Os:             nil,
		OsVersion:      nil,
		Architecture:   nil,
		LogicalCpus:    nil,
		Memory:         nil,
		HostUptimeSecs: 0,
		NodeUptimeSecs: 0,
		Timezone:       nil,
		Tools:          nil,
	}
}
func (value *HostInfo) UnmarshalJSON(data []byte) error { return Decode(data, value) }
func (value *HostInfo) DecodeJSON(decoder *Decoder) error {
	type plain HostInfo
	decoded := plain{}
	if err := decoder.Object(&decoded, []string{"node", "sampled_at_ms", "host_uptime_secs", "node_uptime_secs"}, false); err != nil {
		return err
	}
	*value = HostInfo(decoded)
	return nil
}
func (value HostInfo) MarshalJSON() ([]byte, error) {
	type plain HostInfo
	encoded := plain(value)
	return json.Marshal(encoded)
}

type MemoryUsage struct {
	TotalBytes     uint64 `json:"total_bytes"`
	AvailableBytes uint64 `json:"available_bytes"`
	FreeBytes      uint64 `json:"free_bytes"`
}

func DefaultMemoryUsage() MemoryUsage {
	return MemoryUsage{
		TotalBytes:     0,
		AvailableBytes: 0,
		FreeBytes:      0,
	}
}
func (value *MemoryUsage) UnmarshalJSON(data []byte) error {
	type plain MemoryUsage
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"total_bytes", "available_bytes", "free_bytes"}, false); err != nil {
		return err
	}
	*value = MemoryUsage(decoded)
	return nil
}
func (value MemoryUsage) MarshalJSON() ([]byte, error) {
	type plain MemoryUsage
	encoded := plain(value)
	return json.Marshal(encoded)
}
