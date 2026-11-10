package operations

import (
	"encoding/json"
	"strings"
	"unicode"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type ModeSelection struct {
	Package     protocol.PluginReference
	Declaration protocol.PluginModesDeclaration
}

func (r *Resources) SelectedMode(config protocol.SessionConfig) (*ModeSelection, error) {
	var selected *ModeSelection
	for _, p := range r.packages {
		if len(p.Modes) == 0 {
			continue
		}
		if err := validModes(p); err != nil {
			return nil, err
		}
		for _, mode := range p.Modes {
			var value string
			if raw := config.Options[p.Reference.Name][mode.Option]; len(raw) > 0 && json.Unmarshal(raw, &value) != nil {
				return nil, resourceFault(protocol.ErrorCodeInvalidRequest, "invalid conversation mode option")
			}
			if value != mode.ID {
				continue
			}
			if selected != nil || config.Mode == protocol.WorkModeFlagship {
				return nil, resourceFault(protocol.ErrorCodeInvalidRequest, "only one conversation mode can be selected")
			}
			if !mode.Permissions.Write && config.Mode != protocol.WorkModePlan {
				return nil, resourceFault(protocol.ErrorCodePermissionDenied, "conversation mode does not allow writing")
			}
			selected = &ModeSelection{Package: p.Reference, Declaration: mode}
		}
	}
	return selected, nil
}

func validModes(p Package) error {
	invalid := func() error {
		return resourceFault(protocol.ErrorCodeInvalidRequest, "invalid conversation mode declaration")
	}
	if p.Scope != protocol.PluginScopeHost || len(p.Modes) > 16 {
		return invalid()
	}
	var schema struct {
		Properties map[string]struct {
			Kind    string          `json:"type"`
			Default json.RawMessage `json:"default"`
			Choices []string        `json:"enum"`
		} `json:"properties"`
	}
	if json.Unmarshal(p.Options, &schema) != nil {
		return invalid()
	}
	ids := map[string]bool{}
	for _, mode := range p.Modes {
		if mode.ID == "" || len(mode.ID) > 64 || strings.ContainsFunc(mode.ID, func(r rune) bool {
			return r > 127 || !unicode.IsLetter(r) && !unicode.IsDigit(r) && r != '_' && r != '-'
		}) || ids[mode.ID] || mode.Option == "" || len(mode.Option) > 64 || strings.ContainsFunc(mode.Option, func(r rune) bool { return r > 127 || !unicode.IsLetter(r) && !unicode.IsDigit(r) && r != '_' }) || strings.TrimSpace(mode.Instructions) == "" || len(mode.Instructions) > 16*1024 || strings.ContainsRune(mode.Instructions, 0) {
			return invalid()
		}
		ids[mode.ID] = true
		field, ok := schema.Properties[mode.Option]
		var defaultValue string
		if !ok || field.Kind != "string" || json.Unmarshal(field.Default, &defaultValue) != nil || defaultValue != "" {
			return invalid()
		}
		empty, found := false, false
		for _, choice := range field.Choices {
			if choice == "" {
				empty = true
				continue
			}
			declared := false
			for _, other := range p.Modes {
				if other.Option == mode.Option && other.ID == choice {
					declared = true
				}
			}
			if !declared {
				return invalid()
			}
			if choice == mode.ID {
				found = true
			}
		}
		if !empty || !found {
			return invalid()
		}
	}
	return nil
}
