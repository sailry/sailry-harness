package protocol

import "encoding/json"

// ConfigurationOrigin identifies a read-only personal configuration replica.
type ConfigurationOrigin struct {
	Node     NodeID `json:"node"`
	Revision uint64 `json:"revision"`
}

type ConfigurationFollowTarget struct {
	Host          NodeID `json:"host"`
	Revision      uint64 `json:"revision"`
	Configuration bool   `json:"configuration"`
	Secrets       bool   `json:"secrets"`
	Error         *Fault `json:"error,omitempty"`
}

type ConfigurationFollowRecord struct {
	ID          string          `json:"id"`
	Revision    uint64          `json:"revision"`
	Digest      string          `json:"digest"`
	Body        json.RawMessage `json:"body,omitempty"`
	Secret      *Secret         `json:"secret,omitempty"`
	ExpiresAtMs *uint64         `json:"expires_at_ms,omitempty"`
}

type ConfigurationManifest struct {
	Source   NodeID                      `json:"source"`
	Category string                      `json:"category"`
	Revision uint64                      `json:"revision"`
	Records  []ConfigurationFollowRecord `json:"records"`
}

type ConfigurationPackageStatus struct {
	Available bool `json:"available"`
}

type ConfigurationFollowIssue struct {
	Category string `json:"category"`
	ID       string `json:"id"`
	State    string `json:"state"`
	Message  string `json:"message"`
}

type ConfigurationMCPLocal struct {
	Plugin string `json:"plugin"`
	Server string `json:"server"`
}

type ConfigurationFollowStatus struct {
	MCPLocal []ConfigurationMCPLocal     `json:"mcp_local,omitempty"`
	User     *User                       `json:"user,omitempty"`
	Source   *NodeID                     `json:"source,omitempty"`
	Revision uint64                      `json:"revision"`
	Targets  []ConfigurationFollowTarget `json:"targets"`
	Issues   []ConfigurationFollowIssue  `json:"issues"`
}

// ConfigurationFollowCommand is one typed action family. Authenticated caller
// membership chooses the local user; its payload never supplies a user identity.
type ConfigurationFollowCommand struct {
	Action           string                     `json:"action"`
	Source           *NodeID                    `json:"source,omitempty"`
	Target           *ConfigurationFollowTarget `json:"target,omitempty"`
	ExpectedRevision uint64                     `json:"expected_revision,omitempty"`
	Manifest         *ConfigurationManifest     `json:"manifest,omitempty"`
	Page             *ConfigurationSnapshotPage `json:"page,omitempty"`
	Category         string                     `json:"category,omitempty"`
	Plugin           string                     `json:"plugin,omitempty"`
	Server           string                     `json:"server,omitempty"`
	Local            bool                       `json:"local,omitempty"`
	Stream           *StreamID                  `json:"stream,omitempty"`
	Package          *PluginReference           `json:"package,omitempty"`
	KVReceipt        *PersonalKVReceipt         `json:"kv_receipt,omitempty"`
	KVSnapshot       *PersonalKVSnapshot        `json:"kv_snapshot,omitempty"`
	KVRead           *PersonalKVRead            `json:"kv_read,omitempty"`
}

func (value *ConfigurationFollowCommand) UnmarshalJSON(data []byte) error { return Decode(data, value) }
func (value *ConfigurationFollowCommand) DecodeJSON(decoder *Decoder) error {
	type plain ConfigurationFollowCommand
	var decoded plain
	if err := decoder.Object(&decoded, []string{"action"}, false); err != nil {
		return err
	}
	*value = ConfigurationFollowCommand(decoded)
	return nil
}

func init() {
	commandVariants["configuration_follow"] = func() any { return new(ConfigurationFollowCommand) }
	outputVariants["configuration_follow"] = func() any { return new(ConfigurationFollowStatus) }
	outputVariants["configuration_manifest"] = func() any { return new(ConfigurationManifest) }
	outputVariants["configuration_package"] = func() any { return new(ConfigurationPackageStatus) }
	outputVariants["configuration_snapshot"] = func() any { return new(ConfigurationSnapshotProgress) }
	eventVariants["configuration_follow_changed"] = func() any { return new(ConfigurationFollowStatus) }
}
