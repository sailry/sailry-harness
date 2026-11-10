package protocol

import (
	"bytes"
	"encoding/json"
	"math"
	"slices"
	"strconv"
	"strings"
	"unicode"
)

const (
	PluginUiMaxEntries    = 32
	PluginUiMaxChoices    = 256
	PluginUiMaxStateBytes = 64 * 1024
)

func pluginUiIdentifier(value string) bool {
	return value != "" && len(value) <= 128 && !strings.ContainsFunc(value, func(character rune) bool {
		return !(character >= 'a' && character <= 'z' || character >= 'A' && character <= 'Z' || character >= '0' && character <= '9' || strings.ContainsRune("._-", character))
	})
}

func pluginUiIcon(value *string) bool {
	if value == nil {
		return true
	}
	return pluginUiIdentifier(*value) && !strings.Contains(*value, ".") || (PluginDesktopIcon{Kind: "Name", Data: *value}).Valid()
}

func pluginUiCommand(value PluginUiCommand) bool {
	if value.Name == "" || len(value.Name) > 64 || strings.ContainsFunc(value.Name, func(character rune) bool {
		return !(character >= 'a' && character <= 'z' || character >= '0' && character <= '9' || character == '_' || character == '-')
	}) {
		return false
	}
	return !slices.Contains([]string{"model", "reasoning", "plan", "code", "compact", "review", "test", "explain"}, value.Name)
}

func ValidPluginUiContributions(declarations []PluginUiContribution) bool {
	if len(declarations) > PluginUiMaxEntries {
		return false
	}
	ids, commands := map[string]bool{}, map[string]bool{}
	panels := 0
	for _, declaration := range declarations {
		if ids[declaration.ID] || !declaration.Valid() {
			return false
		}
		ids[declaration.ID] = true
		if command := declaration.Command; command != nil {
			if commands[command.Name] {
				return false
			}
			commands[command.Name] = true
		}
		if declaration.Kind == "popover" || (declaration.Kind == "indicator" || declaration.Kind == "metric") && declaration.Handler != nil {
			panels++
		}
	}
	return panels <= 1
}

func (declaration PluginUiContribution) Valid() bool {
	if !pluginUiIdentifier(declaration.ID) || !declaration.Label.Valid() || !pluginUiIcon(declaration.Icon) {
		return false
	}
	if declaration.Intent != nil && (!pluginUiIdentifier(*declaration.Intent) || declaration.Kind != "button" && declaration.Kind != "popover") {
		return false
	}
	if declaration.Kind == "popover" && !slices.Contains([]PluginUiSlot{"composer", "context", "status", "header"}, declaration.Slot) {
		return false
	}
	if (declaration.Slot == "status" || declaration.Slot == "header") && declaration.Kind != "button" && declaration.Kind != "popover" {
		return false
	}
	if declaration.Slot == "commands" && declaration.Command == nil {
		return false
	}
	if declaration.Slot == "project_menu" && declaration.Kind != "button" && declaration.Kind != "menu" {
		return false
	}
	if declaration.Width != nil && (*declaration.Width < 240 || *declaration.Width > 480 || declaration.Kind != "metric" && !(declaration.Kind == "popover" && declaration.Slot == "header")) {
		return false
	}
	if declaration.Hover != nil && (declaration.Kind != "popover" || declaration.Slot != "header" || declaration.Hover.OpenMs > 2000 || declaration.Hover.CloseMs > 2000) {
		return false
	}
	if command := declaration.Command; command != nil {
		if !pluginUiCommand(*command) || !slices.Contains([]PluginUiSlot{"composer", "context", "status", "commands"}, declaration.Slot) {
			return false
		}
		switch command.Kind {
		case "message":
			if declaration.Kind != "button" || declaration.Slot != "commands" {
				return false
			}
		case "invoke":
			if !slices.Contains([]PluginUiKind{"button", "toggle", "popover", "picker"}, declaration.Kind) {
				return false
			}
		default:
			return false
		}
	}
	switch declaration.Kind {
	case "metric":
		return slices.Contains([]PluginUiSlot{"statistics", "footer", "provider"}, declaration.Slot) &&
			(declaration.Handler == nil || declaration.Slot == "statistics" && pluginUiIdentifier(*declaration.Handler)) && len(declaration.Choices) == 0
	case "indicator":
		return declaration.Slot == "composer" && (declaration.Handler == nil || pluginUiIdentifier(*declaration.Handler)) && len(declaration.Choices) == 0
	case "button", "popover", "toggle", "select", "menu", "picker":
		return !slices.Contains([]PluginUiSlot{"statistics", "footer", "provider"}, declaration.Slot) &&
			declaration.Handler != nil && pluginUiIdentifier(*declaration.Handler) &&
			validPluginUiChoices(declaration.Choices, declaration.Kind == "menu") &&
			(slices.Contains([]PluginUiKind{"select", "picker", "menu"}, declaration.Kind) || len(declaration.Choices) == 0)
	default:
		return false
	}
}

func validPluginUiChoices(choices []PluginUiChoice, nested bool) bool {
	count := 0
	ids := map[string]bool{}
	valid := func(choice PluginUiChoice) bool {
		count++
		if count > PluginUiMaxChoices || ids[choice.ID] || choice.ID == "" || len(choice.ID) > 512 || strings.ContainsFunc(choice.ID, unicode.IsControl) ||
			!choice.Label.Valid() || !pluginUiIcon(choice.Icon) || choice.Description != nil && !choice.Description.Valid() || choice.Group != nil && !choice.Group.Valid() {
			return false
		}
		ids[choice.ID] = true
		return true
	}
	for _, choice := range choices {
		if !valid(choice) || !nested && len(choice.Children) != 0 {
			return false
		}
		for _, child := range choice.Children {
			if len(child.Children) != 0 || !valid(child) {
				return false
			}
		}
	}
	return true
}

func (scope PluginUiProviderScope) Valid() bool {
	provider, credential, found := strings.Cut(scope.Revision, ":")
	if !found {
		return false
	}
	for _, revision := range []string{provider, credential} {
		value, err := strconv.ParseUint(revision, 10, 64)
		if err != nil || strconv.FormatUint(value, 10) != revision {
			return false
		}
	}
	return true
}

func (state PluginUiState) Valid(declaration PluginUiContribution) bool {
	if state.ID != declaration.ID || (declaration.Slot == "provider") != (state.Scope != nil) ||
		state.Scope != nil && !state.Scope.Valid() || state.Label != nil && !state.Label.Valid() || !pluginUiIcon(state.Icon) ||
		state.Dropdown && declaration.Kind != "button" && declaration.Kind != "popover" {
		return false
	}
	if state.Choices != nil && (!validPluginUiChoices(*state.Choices, declaration.Kind == "menu") || !slices.Contains([]PluginUiKind{"select", "picker", "menu"}, declaration.Kind)) {
		return false
	}
	if len(state.Segments) > 16 || len(state.Segments) != 0 && declaration.Slot != "status" && declaration.Slot != "model" || len(state.Details) > PluginUiMaxEntries {
		return false
	}
	for _, segment := range state.Segments {
		if !segment.Label.Valid() {
			return false
		}
	}
	for _, detail := range state.Details {
		if !detail.Label.Valid() || len(detail.Value) > 4096 {
			return false
		}
		if detail.Progress != nil {
			value, err := detail.Progress.Float64()
			if err != nil || math.IsNaN(value) || math.IsInf(value, 0) || value < 0 || value > 100 {
				return false
			}
		}
	}
	var value any
	decoder := json.NewDecoder(bytes.NewReader(state.Value))
	decoder.UseNumber()
	if decoder.Decode(&value) != nil {
		return false
	}
	if _, array := value.([]any); array {
		return false
	}
	if declaration.Kind == "indicator" {
		if value != nil {
			var indicator PluginUiIndicator
			if json.Unmarshal(state.Value, &indicator) != nil || indicator.Open && declaration.Handler == nil || len(indicator.Hint) > 4096 ||
				indicator.Percent != nil && (math.IsNaN(*indicator.Percent) || math.IsInf(*indicator.Percent, 0) || *indicator.Percent < 0) {
				return false
			}
		}
	} else if _, object := value.(map[string]any); object {
		return false
	}
	if text, ok := value.(string); ok && len(text) > 4096 {
		return false
	}
	if declaration.Kind == "toggle" || declaration.Kind == "popover" {
		if _, boolean := value.(bool); !boolean && value != nil {
			return false
		}
	}
	return declaration.Kind == "metric" || declaration.Slot == "status" || len(state.Details) == 0
}

func (scope PluginUiStatusScope) Valid() bool {
	return scope.Worktree != nil || scope.Session != nil
}

func ValidPluginUiStatusState(state PluginUiState, declaration PluginUiContribution) bool {
	return declaration.Slot == PluginUiSlotStatus && state.Valid(declaration) &&
		state.ReplyTo == nil && state.Scope == nil && bytes.Equal(bytes.TrimSpace(state.Value), []byte("null")) &&
		state.Choices == nil
}
