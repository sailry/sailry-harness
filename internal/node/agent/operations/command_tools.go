package operations

import (
	_ "embed"
	"encoding/json"
	"sync"

	"github.com/sailry/sailry-harness/internal/protocol"
)

//go:embed command_tools.json
var commandToolSpecification []byte

// CommandSpec pairs one built-in command tool with its conversation display.
type CommandSpec struct {
	Name string `json:"name"`
	Spec
	Presentation protocol.ToolPresentation `json:"presentation"`
	Display      protocol.ToolDisplay      `json:"display"`
}

type commandCatalog struct {
	Tools   []CommandSpec                               `json:"tools"`
	Notices map[string]protocol.PluginDesktopNavigation `json:"notices"`
}

var commandTools = sync.OnceValues(func() (commandCatalog, error) {
	var catalog commandCatalog
	if err := json.Unmarshal(commandToolSpecification, &catalog); err != nil {
		return catalog, err
	}
	for i := range catalog.Tools {
		catalog.Tools[i].ReadOnly, catalog.Tools[i].Concurrent = Policy(catalog.Tools[i].Operation)
	}
	return catalog, nil
})

// CommandSpecs supplies the session command tools independently of installed
// packages. They share policies with the corresponding package operations.
func CommandSpecs() ([]CommandSpec, error) {
	catalog, err := commandTools()
	return catalog.Tools, err
}

// CommandContent describes inspectable output from the core command tools.
func CommandContent(output protocol.Output) *protocol.ToolContent {
	content := protocol.ToolContent{Version: 1}
	var stdout, stderr protocol.ProcessCapture
	switch value := output.Data.(type) {
	case protocol.ProcessCompletion:
		stdout, stderr = value.Stdout, value.Stderr
	case protocol.ProcessSnapshot:
		stdout, stderr = value.Stdout, value.Stderr
	case protocol.ProcessOutputOutputPage, protocol.ToolOutputOutputPage:
		content.Blocks = []protocol.ToolContentBlock{{Kind: "text", Data: &protocol.ToolContentText{Path: "/data/text"}}}
		return &content
	default:
		return nil
	}
	catalog, err := commandTools()
	if err != nil {
		return nil
	}
	for _, stream := range []struct {
		name    string
		capture protocol.ProcessCapture
	}{{"stdout", stdout}, {"stderr", stderr}} {
		line := protocol.ToolContentText{Path: "/data/" + stream.name + "/text"}
		if stream.capture.InvalidUtf8 {
			line.Notices = append(line.Notices, catalog.Notices["encoding"])
		}
		content.Blocks = append(content.Blocks, protocol.ToolContentBlock{Kind: "text", Data: &line})
	}
	return &content
}

// IsCommandTool identifies a core session command by its model-facing name.
func IsCommandTool(name string) bool {
	switch name {
	case "run_command", "read_command", "stop_command", "write_command":
		return true
	}
	return false
}

var legacyCommandTools = map[string]string{
	Alias("commands", "run_command"):   "run_command",
	Alias("commands", "read_command"):  "read_command",
	Alias("commands", "stop_command"):  "stop_command",
	Alias("commands", "write_command"): "write_command",
}

// LegacyCommandTool resolves the former commands package's canonical alias.
func LegacyCommandTool(alias string) string { return legacyCommandTools[alias] }
