package mcp

import "testing"

func TestAliasDisplayReadsServerAndTool(t *testing.T) {
	for _, row := range []struct {
		alias, label string
		ok           bool
	}{
		{Alias("github", "github", "actions_list"), "mcp_github actions_list", true},
		{Alias("tools", "fetch", "page"), "mcp_fetch page", true},
		// A server named after its package, then more, is not a repeat.
		{Alias("foo", "foo_bar", "baz"), "mcp_foo_bar baz", true},
		{Alias("foo", "foo", "bar_baz"), "mcp_foo bar_baz", true},
		// A package and server of one name with underscores is read once.
		{Alias("my_pkg", "my_pkg", "run"), "mcp_my_pkg run", true},
		// A cut label keeps its one repeat, and two are unclear.
		{Alias("workspace", "workspace", "a_tool_with_a_very_long_name"), "mcp_workspace a_tool_with_", true},
		{Alias("a", "a", "a_a_tool_with_a_long_enough_name"), "mcp_a_a_a_a_tool_with_a_long_enough_", true},
		// A name the label had to change cannot be checked against the digest.
		{Alias("my.pkg", "my.pkg", "run"), "mcp_my_pkg run", true},
		{"mcp_tools_fetch_page_0123456789abcdef01234567", "mcp_tools_fetch_page", true},
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
