package engine

import (
	_ "embed"
	"encoding/json"
	"slices"
	"sync"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

//go:embed i18n/core-tools.json
var coreToolLabels []byte

var coreLabels = sync.OnceValue(func() map[string]map[string]string {
	var values map[string]map[string]string
	if err := json.Unmarshal(coreToolLabels, &values); err != nil {
		panic(err)
	}
	return values
})

func (r *Runtime) captureCorePresentation(record *canonical.Record) {
	message := record.Message()
	if message == nil {
		return
	}
	r.mu.Lock()
	owner := r.active[record.Turn]
	r.mu.Unlock()
	if owner == nil {
		return
	}
	for _, block := range message.Content {
		name := ""
		switch value := block.Data.(type) {
		case *canonical.ToolRequest:
			name = value.Name
		case *canonical.ToolResponse:
			name = value.Name
		}
		if !slices.Contains([]string{"spawn_agent", "guide_agent", "wait_agent", "load_skill", "read_skill_resource"}, name) || findTool(owner.tools, name) == nil {
			continue
		}
		if record.Metadata.Presentation == nil {
			record.Metadata.Presentation = &canonical.Presentation{}
		}
		p := record.Metadata.Presentation
		if p.Tools == nil {
			p.Tools = map[string]protocol.ToolPresentation{}
		}
		if p.Displays == nil {
			p.Displays = map[string]protocol.ToolDisplay{}
		}
		if p.Groupings == nil {
			p.Groupings = map[string]protocol.ToolGrouping{}
		}
		p.Tools[name] = protocol.ToolPresentationDetails
		label := coreLabels()["tool_"+name]
		display := protocol.ToolDisplay{Label: label["en"], Locales: map[string]string{}}
		for locale, value := range label {
			if locale != "en" {
				display.Locales[locale] = value
			}
		}
		field := func(path string) *protocol.ToolDisplayField { return &protocol.ToolDisplayField{Path: path} }
		display.Input = &protocol.ToolDisplayInput{}
		switch name {
		case "spawn_agent":
			display.Input.Summary, display.Input.Context, display.Input.Content = field("/title"), field("/role"), field("/task")
			p.Groupings[name] = protocol.ToolGroupingStandalone
			p.Delegations = append(p.Delegations, name)
		case "guide_agent", "wait_agent":
			display.Input.Summary = field("/session")
			format := protocol.ToolDisplayInputFormatSession
			display.Input.Summary.Format = &format
			if name == "guide_agent" {
				display.Input.Content = field("/message")
			}
		default:
			display.Input.Summary = field("/skill")
			p.Tools[name] = protocol.ToolPresentationSummary
		}
		if name == "spawn_agent" || name == "guide_agent" || name == "wait_agent" {
			display.Icon = &protocol.PluginDesktopIcon{Kind: "Name", Data: "reicon:users/users2"}
		}
		p.Displays[name] = display
	}
}
