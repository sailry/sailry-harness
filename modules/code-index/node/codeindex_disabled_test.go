//go:build sailry_no_code_index

package codeindex_test

import (
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestCodeIndexCommandsUnavailable(t *testing.T) {
	t.Parallel()
	node, tree := indexNodeFixture(t)
	name := "main"
	commands := []protocol.Command{
		{Kind: "search_code", Data: protocol.CommandSearchCodePayload{Worktree: tree, Query: protocol.CodeIndexSearch{Query: name, Limit: 1}}},
		{Kind: "outline_code", Data: protocol.CommandOutlineCodePayload{Worktree: tree, Path: "main.go"}},
		{Kind: "graph_code", Data: protocol.CommandGraphCodePayload{Worktree: tree, Query: protocol.CodeIndexGraph{View: protocol.CodeIndexGraphViewCallers, Name: &name, Limit: 1}}},
		{Kind: "cancel_code_index_query", Data: protocol.CommandCancelCodeIndexQueryPayload{Request: protocol.NewRequestID()}},
		{Kind: "read_code_index_settings"},
		{Kind: "save_code_index_settings", Data: protocol.CommandSaveCodeIndexSettingsPayload{Settings: protocol.CodeIndexSettings{Enabled: true, Persistent: new(true)}}},
	}
	for _, command := range commands {
		result := dispatchIndexCommand(t, node, command)
		if result.Fault == nil || result.Fault.Code != protocol.ErrorCodeUnavailable {
			t.Errorf("%s = %+v", command.Kind, result)
		}
	}
}
