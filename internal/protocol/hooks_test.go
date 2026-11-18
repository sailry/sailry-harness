package protocol

import (
	"encoding/json"
	"testing"
)

func TestNodeHookCommandScope(t *testing.T) {
	read, err := json.Marshal(Command{Kind: "read_hooks"})
	if err != nil || string(read) != `{"kind":"read_hooks"}` {
		t.Fatalf("Node hook read: %s, %v", read, err)
	}
	var command Command
	if err = json.Unmarshal(read, &command); err != nil || command.Kind != "read_hooks" || command.Data != nil {
		t.Fatalf("decoded Node hook read: %+v, %v", command, err)
	}
	validSave := `{"kind":"save_hooks","data":{"settings":{"revision":0,"enabled":false},"expected_revision":0}}`
	if err = json.Unmarshal([]byte(validSave), &command); err != nil {
		t.Fatalf("Node hook save: %v", err)
	}
	for _, legacy := range []string{
		`{"kind":"read_hooks","data":{"worktree":"12345678-1234-1234-1234-123456789abc"}}`,
		`{"kind":"save_hooks","data":{"settings":{"revision":0,"enabled":false},"expected_revision":0,"worktree":"12345678-1234-1234-1234-123456789abc"}}`,
	} {
		if err = json.Unmarshal([]byte(legacy), &command); err == nil {
			t.Fatalf("obsolete worktree hook command was accepted: %s", legacy)
		}
	}
}
