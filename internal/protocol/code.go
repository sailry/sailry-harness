// Hand-maintained v1 protocol declarations; keep them consistent with the fixtures and tolerance tests.

package protocol

import (
	"encoding/json"
)

type CodeStartup struct {
	Worktree   WorktreeID `json:"worktree"`
	Root       string     `json:"root"`
	Server     string     `json:"server"`
	Executable string     `json:"executable"`
	Revision   string     `json:"revision"`
}

func DefaultCodeStartup() CodeStartup {
	return CodeStartup{
		Worktree:   WorktreeID{},
		Root:       "",
		Server:     "",
		Executable: "",
		Revision:   "",
	}
}
func (value *CodeStartup) UnmarshalJSON(data []byte) error {
	type plain CodeStartup
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree", "root", "server", "executable", "revision"}, true); err != nil {
		return err
	}
	*value = CodeStartup(decoded)
	return nil
}
func (value CodeStartup) MarshalJSON() ([]byte, error) {
	type plain CodeStartup
	encoded := plain(value)
	return json.Marshal(encoded)
}
