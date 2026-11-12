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
	// Mode is the session mode every session must be in before a prompt is
	// sent. An agent that does not offer it is refused. Without it the
	// session keeps the agent's own mode.
	Mode *string `json:"mode,omitempty"`
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

// PluginModelAdapterClaudeCode selects the Claude Code route: the adapter's
// transport is the user's own installed `claude`, run by the Node as a
// per-step model. The Node owns the command line; the program it names
// declares no arguments and no files.
type PluginModelAdapterClaudeCode struct {
	Program string `json:"program"`
	// SignIn tells the user how to sign in to Claude Code.
	SignIn string `json:"sign_in"`
	// Models are the models the channel offers. Claude Code lists none
	// without a request, so the package names them.
	Models []PluginModelAdapterClaudeCodeModel `json:"models"`
}

func (value *PluginModelAdapterClaudeCode) UnmarshalJSON(data []byte) error {
	type plain PluginModelAdapterClaudeCode
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"program", "sign_in", "models"}, true); err != nil {
		return err
	}
	*value = PluginModelAdapterClaudeCode(decoded)
	return nil
}
func (value PluginModelAdapterClaudeCode) MarshalJSON() ([]byte, error) {
	type plain PluginModelAdapterClaudeCode
	encoded := plain(value)
	encoded.Models = normalizeCollections(encoded.Models)
	return json.Marshal(encoded)
}

// PluginModelAdapterClaudeCodeModel is one model name `claude --model`
// accepts, an alias or a full name.
type PluginModelAdapterClaudeCodeModel struct {
	ID   string  `json:"id"`
	Name *string `json:"name,omitempty"`
	// Context and Output are the model's token limits where the package
	// knows them.
	Context *uint32 `json:"context,omitempty"`
	Output  *uint32 `json:"output,omitempty"`
	// Efforts are the effort levels the model takes, lowest first.
	Efforts []string `json:"efforts,omitempty"`
}

func (value *PluginModelAdapterClaudeCodeModel) UnmarshalJSON(data []byte) error {
	type plain PluginModelAdapterClaudeCodeModel
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id"}, true); err != nil {
		return err
	}
	*value = PluginModelAdapterClaudeCodeModel(decoded)
	return nil
}
