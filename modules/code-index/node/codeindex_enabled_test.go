//go:build !sailry_no_code_index

package codeindex_test

import (
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestCodeIndexSettingsApplyBeforeCompletion(t *testing.T) {
	node, tree := indexNodeFixture(t)
	indexCommand(t, node, protocol.Command{Kind: "save_code_index_settings", Data: protocol.CommandSaveCodeIndexSettingsPayload{Settings: protocol.CodeIndexSettings{Enabled: false}}})
	result := dispatchIndexCommand(t, node, protocol.Command{Kind: "search_code", Data: protocol.CommandSearchCodePayload{Worktree: tree, Query: protocol.CodeIndexSearch{Query: "main", Limit: 1}}})
	if result.Fault == nil || result.Fault.Code != protocol.ErrorCodeNotConfigured {
		t.Fatalf("search after disabling = %#v", result)
	}
}
