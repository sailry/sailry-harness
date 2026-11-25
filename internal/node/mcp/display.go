package mcp

import (
	"regexp"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// aliasPattern matches Alias: the package, server and tool joined and cut to
// 32 characters, then a 24-digit digest.
var aliasPattern = regexp.MustCompile(`^mcp_([A-Za-z0-9_-]{1,32})_[0-9a-f]{24}$`)

// Display is how a call of a listed tool without a declared display is
// shown: by its server and tool name, not by its alias, under the tool icon
// that both the Desktop and the mobile app bundle.
func Display(server, tool string) protocol.ToolDisplay {
	return display("mcp_" + server + " " + tool)
}

// AliasDisplay shows a call recorded before listed tools carried a display.
// The digest is dropped and a package that repeats its server's name is read
// once, so mcp_github_github_actions_list_<digest> reads
// "mcp_github actions_list". The alias cannot say where other names split,
// so the rest stays as recorded.
func AliasDisplay(alias string) (protocol.ToolDisplay, bool) {
	match := aliasPattern.FindStringSubmatch(alias)
	if match == nil {
		return protocol.ToolDisplay{}, false
	}
	label := match[1]
	if first, rest, found := strings.Cut(label, "_"); found {
		if repeated, tool, found := strings.Cut(rest, "_"); found && repeated == first && tool != "" {
			return Display(first, tool), true
		}
	}
	return display("mcp_" + label), true
}

func display(label string) protocol.ToolDisplay {
	return protocol.ToolDisplay{Label: label, Icon: &protocol.PluginDesktopIcon{Kind: "Name", Data: "reicon:ui/puzzle-piece"}, Locales: map[string]string{}}
}
