package mcp

import "testing"

func TestAliasDisplayReadsServerAndTool(t *testing.T) {
	for _, row := range []struct {
		alias, label string
		ok           bool
	}{
		{Alias("github", "github", "actions_list"), "mcp_github actions_list", true},
		{Alias("tools", "fetch", "page"), "mcp_tools_fetch_page", true},
		{Alias("workspace", "workspace", "a_tool_with_a_very_long_name"), "mcp_workspace a_tool_with_", true},
		{"mcp_short", "", false},
		{"files_read_file", "", false},
	} {
		display, ok := AliasDisplay(row.alias)
		if ok != row.ok || display.Label != row.label {
			t.Errorf("%s: label %q ok %v, want %q %v", row.alias, display.Label, ok, row.label, row.ok)
		}
		if ok && (display.Icon == nil || display.Icon.Kind != "Name" || display.Icon.Data != "reicon:ui/puzzle-piece") {
			t.Errorf("%s: icon %+v", row.alias, display.Icon)
		}
	}
	if display := Display("github", "actions_list"); display.Label != "mcp_github actions_list" || display.Icon == nil || display.Icon.Data != "reicon:ui/puzzle-piece" {
		t.Errorf("listed display = %+v", display)
	}
}
