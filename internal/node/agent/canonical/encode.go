package canonical

import (
	"bytes"
	"encoding/json"
	"slices"
)

func nilNull(raw json.RawMessage) json.RawMessage {
	if bytes.Equal(bytes.TrimSpace(raw), []byte("null")) {
		return nil
	}
	return raw
}
func (v Usage) MarshalJSON() ([]byte, error) {
	type plain Usage
	p := plain(v)
	p.Provider = nilNull(p.Provider)
	return json.Marshal(p)
}
func (v ToolRequest) MarshalJSON() ([]byte, error) {
	type plain ToolRequest
	p := plain(v)
	p.ToolMeta = nilNull(p.ToolMeta)
	return json.Marshal(p)
}
func (v ToolError) MarshalJSON() ([]byte, error) {
	type plain ToolError
	p := plain(v)
	p.Data = nilNull(p.Data)
	return json.Marshal(p)
}
func (v AssetRef) MarshalJSON() ([]byte, error) {
	type plain AssetRef
	p := plain(v)
	p.Annotations = nilNull(p.Annotations)
	return json.Marshal(p)
}
func (v RedactedThinking) MarshalJSON() ([]byte, error) {
	type plain RedactedThinking
	p := plain(v)
	if p.Data == nil {
		p.Data = []string{}
	}
	if p.Before == nil {
		p.Before = []*uint64{}
	}
	return json.Marshal(p)
}
func (v SubagentsCancelled) MarshalJSON() ([]byte, error) {
	type plain SubagentsCancelled
	p := plain(v)
	if p.Sessions == nil {
		p.Sessions = []string{}
	}
	return json.Marshal(p)
}
func (v Presentation) MarshalJSON() ([]byte, error) {
	type plain Presentation
	p := plain(v)
	p.Delegations = slices.Clone(p.Delegations)
	slices.Sort(p.Delegations)
	p.Delegations = slices.Compact(p.Delegations)
	return json.Marshal(p)
}

func (v Context) MarshalJSON() ([]byte, error) {
	type plain Context
	p := plain(v)
	if p.LoadedTools != nil && *p.LoadedTools == nil {
		empty := []string{}
		p.LoadedTools = &empty
	}
	return json.Marshal(p)
}
