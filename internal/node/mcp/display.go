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

// AliasDisplay shows a call recorded before listed tools carried a display,
// as Display shows the tool. A label holds every name when it was not cut:
// the split whose names give the alias's own digest is the tool's. A cut
// label is read by a package that repeats its server's name, as
// mcp_github_github_actions_list_<digest> reads "mcp_github actions_list",
// when exactly one such repeat starts it. Any other label stays as recorded.
func AliasDisplay(alias string) (protocol.ToolDisplay, bool) {
	match := aliasPattern.FindStringSubmatch(alias)
	if match == nil {
		return protocol.ToolDisplay{}, false
	}
	label := match[1]
	if server, tool, found := aliasNames(alias, label); found {
		return Display(server, tool), true
	}
	if server, tool, found := repeatedServer(label); found {
		return Display(server, tool), true
	}
	return display("mcp_" + label), true
}

// aliasNames finds the server and tool whose alias this is among the splits
// of an uncut label at its underscores.
func aliasNames(alias, label string) (server, tool string, found bool) {
	cuts := underscores(label)
	for i, first := range cuts {
		for _, second := range cuts[i+1:] {
			if Alias(label[:first], label[first+1:second], label[second+1:]) == alias {
				return label[first+1 : second], label[second+1:], true
			}
		}
	}
	return "", "", false
}

// repeatedServer reads a label that starts with one name twice, of any
// length, as that server's tool. Two such readings are unclear.
func repeatedServer(label string) (server, tool string, found bool) {
	for _, cut := range underscores(label) {
		name := label[:cut]
		if rest, repeated := strings.CutPrefix(label[cut+1:], name+"_"); name != "" && repeated && rest != "" {
			if found {
				return "", "", false
			}
			server, tool, found = name, rest, true
		}
	}
	return server, tool, found
}

func underscores(label string) []int {
	var cuts []int
	for index := range len(label) {
		if label[index] == '_' {
			cuts = append(cuts, index)
		}
	}
	return cuts
}

func display(label string) protocol.ToolDisplay {
	return protocol.ToolDisplay{Label: label, Icon: &protocol.PluginDesktopIcon{Kind: "Name", Data: "reicon:ui/puzzle-piece"}, Locales: map[string]string{}}
}
