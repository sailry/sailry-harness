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
		manager, catalog := managementLabels[name], slices.Contains(catalogToolNames, name)
		if !slices.Contains([]string{"spawn_agent", "guide_agent", "wait_agent", "load_skill", "read_skill_resource"}, name) && manager == "" && !catalog || findTool(owner.tools, name) == nil {
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
		if manager != "" {
			label = coreLabels()[manager]
		}
		display := protocol.ToolDisplay{Label: label["en"], Locales: map[string]string{}}
		for locale, value := range label {
			if locale != "en" {
				display.Locales[locale] = value
			}
		}
		field := func(path string) *protocol.ToolDisplayField { return &protocol.ToolDisplayField{Path: path} }
		if manager != "" {
			display.Input = &protocol.ToolDisplayInput{Summary: field("/name")}
			display.Output = managementOutput(name)
			p.Displays[name] = display
			continue
		}
		if catalog {
			display.Icon = &protocol.PluginDesktopIcon{Kind: "Name", Data: "reicon:users/users2"}
			switch name {
			case "list_agents", "list_teams":
				p.Tools[name] = protocol.ToolPresentationSummary
			case "add_agent", "add_team":
				display.Input = &protocol.ToolDisplayInput{Summary: field("/name")}
			case "edit_agent", "delete_agent", "edit_team", "delete_team":
				display.Input = &protocol.ToolDisplayInput{Summary: field("/id")}
			}
			p.Displays[name] = display
			continue
		}
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

// managementLabels names the settings label each package manager is shown under.
var managementLabels = map[string]string{"manage_plugins": "settings_plugins", "manage_skills": "settings_skills", "manage_mcp": "settings_mcp"}

// catalogToolNames are the role and team catalog tools.
var catalogToolNames = []string{"list_agents", "add_agent", "edit_agent", "delete_agent", "apply_agents", "list_teams", "add_team", "edit_team", "delete_team", "apply_team"}

// managementOutput previews a package change as its name and resulting
// version, followed by any per-package failures.
func managementOutput(name string) *protocol.ToolDisplayProjectionOutput {
	mutations := `{"any":[{"value":{"source":"arguments","path":"/action"},"equals":"install"},{"value":{"source":"arguments","path":"/action"},"equals":"update"},{"value":{"source":"arguments","path":"/action"},"equals":"uninstall"}]}`
	parts := `{"value":{"source":"arguments","path":"/name"},"when":` + mutations + `}`
	if version := map[string]string{"manage_plugins": "/summary/version", "manage_skills": "/source/registry/version"}[name]; version != "" {
		parts += `,{"value":{"source":"result","path":"` + version + `"},"when":` + mutations + `}`
	}
	parts += `,{"rows":{"value":{"source":"result","path":"/failures"},"separator":"\n","text":{"separator":": ","parts":[{"value":{"source":"item","path":"/name"}},{"value":{"source":"item","path":"/error/code"}},{"value":{"source":"item","path":"/error/message"}}]}}}`
	var output protocol.ToolDisplayProjectionOutput
	if err := json.Unmarshal([]byte(`{"preview":{"separator":" ","parts":[`+parts+`]}}`), &output); err != nil {
		panic(err)
	}
	return &output
}
