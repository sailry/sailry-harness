package engine

import (
	"encoding/json"
	"testing"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// A listed MCP tool's record carries the display the turn built for it, so
// its row reads by server and tool instead of by its internal alias.
func TestCapturedMCPToolKeepsItsDisplay(t *testing.T) {
	t.Parallel()
	f, x := delegationFixture(t)
	if err := f.r.captureTools(x); err != nil {
		t.Fatal(err)
	}
	name := "mcp_github_github_actions_list_a837b30f59cf3f2ef96ec5ff"
	display := protocol.ToolDisplay{Label: "mcp_github actions_list", Icon: &protocol.PluginDesktopIcon{Kind: "Name", Data: "reicon:ui/puzzle-piece"}, Locales: map[string]string{}}
	f.r.mu.Lock()
	x.tools = append(x.tools, Tool{Definition: fantasy.FunctionTool{Name: name}, Display: &display})
	f.r.mu.Unlock()
	call := canonical.ToolRequest{ID: "actions", Name: name, Arguments: json.RawMessage(`{}`)}
	response := canonical.ToolResponse{ID: "actions", Name: name}
	for _, block := range []canonical.Block{{Kind: "tool_request", Data: &call}, {Kind: "tool_response", Data: &response}} {
		record := canonical.NewRecord(x.inv.Run.Turn, "message", canonical.NewMessage(canonical.Assistant, block), f.r.clock.Now())
		f.r.captureCorePresentation(&record)
		p := record.Metadata.Presentation
		if p == nil || p.Displays[name].Label != "mcp_github actions_list" || p.Displays[name].Icon == nil || p.Displays[name].Icon.Data != "reicon:ui/puzzle-piece" {
			t.Fatalf("%s display = %+v", block.Kind, p)
		}
		if _, ok := p.Tools[name]; ok {
			t.Fatalf("%s presentation changed to %q", block.Kind, p.Tools[name])
		}
	}
}
