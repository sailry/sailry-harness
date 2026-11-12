package protocol

import "encoding/json"

// PluginProgramDeclaration names one executable a package may have the Node
// start. The Node resolves Command on its own search path and fills the
// placeholders of Args and Env; a package supplies no path and no shell.
type PluginProgramDeclaration struct {
	ID      string              `json:"id"`
	Command string              `json:"command"`
	Args    []string            `json:"args"`
	Env     map[string]string   `json:"env,omitempty"`
	Files   []PluginProgramFile `json:"files,omitempty"`
}

func DefaultPluginProgramDeclaration() PluginProgramDeclaration {
	return PluginProgramDeclaration{Args: []string{}, Env: map[string]string{}, Files: []PluginProgramFile{}}
}
func (value *PluginProgramDeclaration) UnmarshalJSON(data []byte) error {
	type plain PluginProgramDeclaration
	decoded := plain(DefaultPluginProgramDeclaration())
	if err := DecodeObject(data, &decoded, []string{"id", "command", "args"}, true); err != nil {
		return err
	}
	*value = PluginProgramDeclaration(decoded)
	return nil
}
func (value PluginProgramDeclaration) MarshalJSON() ([]byte, error) {
	type plain PluginProgramDeclaration
	encoded := plain(value)
	encoded.Args = normalizeCollections(encoded.Args)
	return json.Marshal(encoded)
}

// PluginProgramFile is a file the Node writes into the program's own
// directory before the program starts.
type PluginProgramFile struct {
	Path    string `json:"path"`
	Content string `json:"content"`
}

func (value *PluginProgramFile) UnmarshalJSON(data []byte) error {
	type plain PluginProgramFile
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"path", "content"}, true); err != nil {
		return err
	}
	*value = PluginProgramFile(decoded)
	return nil
}

// PluginModelAdapterAcp selects the Agent Client Protocol route: the adapter's
// transport is a declared program instead of HTTP, and it has no script.
type PluginModelAdapterAcp struct {
	Program string `json:"program"`
	// SystemMeta is the dotted `_meta` path of `session/new` that carries the
	// system text. Without it the system text opens the session's first prompt.
	SystemMeta *string `json:"system_meta,omitempty"`
	// ModelArgument selects the model through the program's `{model}`
	// argument, one process per model, instead of a session request.
	ModelArgument bool `json:"model_argument,omitempty"`
	// Efforts maps an effort level to the suffix of the agent's model
	// identifier for that level.
	Efforts map[string]string `json:"efforts,omitempty"`
	// ReplayBytes bounds the text of one prompt.
	ReplayBytes uint32 `json:"replay_bytes"`
	// AuthMethod is the `authenticate` method tried once when the agent
	// requires authentication.
	AuthMethod *string `json:"auth_method,omitempty"`
	// SignIn tells the user how to sign in to the program.
	SignIn string `json:"sign_in"`
	// FailureMarkers are texts by which the agent reports its own failure
	// inside a reply.
	FailureMarkers []string `json:"failure_markers,omitempty"`
}

func (value *PluginModelAdapterAcp) UnmarshalJSON(data []byte) error {
	type plain PluginModelAdapterAcp
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"program", "replay_bytes", "sign_in"}, true); err != nil {
		return err
	}
	*value = PluginModelAdapterAcp(decoded)
	return nil
}
