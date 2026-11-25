package store

import (
	"encoding/json"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/mcp"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// A listed MCP tool's call recorded before calls carried a display reads by
// server and tool for every client; a recorded display still wins.
func TestRecordedMCPCallsReadByServerAndTool(t *testing.T) {
	name := mcp.Alias("github", "github", "actions_list")
	block := canonical.Block{Kind: "tool_request", Data: &canonical.ToolRequest{ID: "actions", Name: name, Arguments: json.RawMessage(`{}`)}}
	raw := rawTool(canonical.Record{}, block)
	if raw == nil || raw.Display == nil || raw.Display.Label != "mcp_github actions_list" || raw.Display.Icon == nil || raw.Display.Icon.Data != "reicon:ui/puzzle-piece" {
		t.Fatalf("recorded call display = %+v", raw)
	}
	if resolved := raw.resolved(false, raw.displayData()); resolved == nil || resolved.Label == nil || resolved.Label.Label != "mcp_github actions_list" {
		t.Fatalf("resolved = %+v", resolved)
	}
	var declared canonical.Record
	declared.Metadata.Presentation = &canonical.Presentation{Displays: map[string]protocol.ToolDisplay{name: {Label: "List runs", Locales: map[string]string{}}}}
	if raw := rawTool(declared, block); raw.Display == nil || raw.Display.Label != "List runs" {
		t.Fatalf("declared display = %+v", raw.Display)
	}
	other := canonical.Block{Kind: "tool_request", Data: &canonical.ToolRequest{ID: "read", Name: "files_read_file", Arguments: json.RawMessage(`{}`)}}
	if raw := rawTool(canonical.Record{}, other); raw.Display != nil {
		t.Fatalf("non-MCP display = %+v", raw.Display)
	}
}
