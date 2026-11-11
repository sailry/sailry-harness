package client

import (
	"encoding/json"
	"net/url"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type ConversationMode struct {
	Package     protocol.PluginReference        `json:"package"`
	Declaration protocol.PluginModesDeclaration `json:"declaration"`
	Enabled     bool                            `json:"enabled"`
}

type ModesView struct {
	Selected *string  `json:"selected"`
	Choices  []string `json:"choices"`
}

// ProjectModesJSON and SelectModeJSON take a session's configuration and the
// mode declarations as a controller holds them: both are a Node's report.
func ProjectModesJSON(config, modes json.RawMessage) (json.RawMessage, error) {
	var captured protocol.SessionConfig
	var declarations []ConversationMode
	if err := protocol.DecodeAs(protocol.PeerData, config, &captured); err != nil {
		return nil, err
	}
	if err := protocol.DecodeAs(protocol.PeerData, modes, &declarations); err != nil {
		return nil, err
	}
	return json.Marshal(ProjectModes(captured, declarations))
}

func SelectModeJSON(config, modes json.RawMessage, key string) (json.RawMessage, error) {
	var captured protocol.SessionConfig
	var declarations []ConversationMode
	if err := protocol.DecodeAs(protocol.PeerData, config, &captured); err != nil {
		return nil, err
	}
	if err := protocol.DecodeAs(protocol.PeerData, modes, &declarations); err != nil {
		return nil, err
	}
	selected := SelectMode(captured, declarations, key)
	if selected == nil {
		return nil, nil
	}
	return json.Marshal(selected)
}

func (mode ConversationMode) Key() string {
	return mode.Package.Name + "/" + mode.Declaration.ID
}

func (mode ConversationMode) selected(config protocol.SessionConfig) bool {
	var option string
	return json.Unmarshal(config.Options[mode.Package.Name][mode.Declaration.Option], &option) == nil && option == mode.Declaration.ID
}

func ProjectModes(config protocol.SessionConfig, modes []ConversationMode) ModesView {
	view := ModesView{Choices: []string{}}
	selected := -1
	count := 0
	for index, mode := range modes {
		matches := mode.selected(config)
		if mode.Enabled || matches {
			view.Choices = append(view.Choices, mode.Key())
		}
		if matches {
			selected = index
			count++
		}
	}
	if count == 0 {
		key := string(config.Mode)
		view.Selected = &key
	}
	if count == 1 && (modes[selected].Declaration.Permissions.Write || config.Mode == protocol.WorkModePlan) {
		key := modes[selected].Key()
		view.Selected = &key
	}
	return view
}

func SelectMode(config protocol.SessionConfig, modes []ConversationMode, key string) *protocol.SessionConfig {
	var selected *ConversationMode
	for index := range modes {
		if modes[index].Enabled && modes[index].Key() == key {
			selected = &modes[index]
			break
		}
	}
	base := protocol.WorkMode(key)
	switch base {
	case protocol.WorkModeCode, protocol.WorkModePlan, protocol.WorkModeFlagship:
	default:
		if selected == nil {
			return nil
		}
		base = protocol.WorkModePlan
		if selected.Declaration.Permissions.Write {
			base = protocol.WorkModeCode
		}
	}
	result := copyValue(config)
	if result.Options == nil {
		result.Options = map[string]map[string]json.RawMessage{}
	}
	for _, mode := range modes {
		if mode.selected(result) {
			result.Options[mode.Package.Name][mode.Declaration.Option] = json.RawMessage(`""`)
		}
	}
	result.Mode = base
	if selected != nil {
		if result.Options[selected.Package.Name] == nil {
			result.Options[selected.Package.Name] = map[string]json.RawMessage{}
		}
		result.Options[selected.Package.Name][selected.Declaration.Option], _ = json.Marshal(selected.Declaration.ID)
	}
	return &result
}

func SessionLink(link string) *protocol.SessionID {
	parsed, err := url.Parse(strings.TrimSpace(link))
	if err != nil || !strings.EqualFold(parsed.Scheme, "sailry") || !strings.EqualFold(parsed.Host, "session") || parsed.User != nil || parsed.RawQuery != "" || parsed.ForceQuery || parsed.Fragment != "" || strings.Contains(link, "#") {
		return nil
	}
	id := strings.TrimPrefix(parsed.EscapedPath(), "/")
	if len(id) != 36 {
		return nil
	}
	session, err := protocol.ParseSessionID(id)
	if err != nil {
		return nil
	}
	return &session
}
