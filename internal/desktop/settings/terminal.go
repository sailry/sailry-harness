package settings

// TerminalMessages are the toasts of the terminal settings' outcomes.
var TerminalMessages = Messages{Failed: "terminal_settings_failed", Conflict: "terminal_settings_conflict", Uncertain: "terminal_settings_unknown"}

// TerminalEnvEditor holds the environment variable editor's draft. Index is
// -1 when adding a variable.
type TerminalEnvEditor struct {
	Open       bool
	Index      int
	Key, Value string
}

type envEditor = TerminalEnvEditor

func (m *Model) TerminalEnvEditor() *TerminalEnvEditor { return &m.env }

// CloseTerminalEditor drops the environment editor when its host changes.
func (m *Model) CloseTerminalEditor() { m.env = TerminalEnvEditor{} }
