package plugins

import (
	"bytes"
	"context"
	"encoding/json"
	"math"
	"slices"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func (registry *Registry) publish(mount *contributionMount, body json.RawMessage) error {
	var states []protocol.PluginUiState
	if len(bytes.TrimSpace(body)) == 0 || bytes.Equal(bytes.TrimSpace(body), []byte("null")) || json.Unmarshal(body, &states) != nil {
		return contributionFault(protocol.ErrorCodeInvalidRequest, "invalid contribution state")
	}
	encoded, err := json.Marshal(states)
	if err != nil || len(states) > protocol.PluginUiMaxEntries || len(encoded) > protocol.PluginUiMaxStateBytes {
		return contributionFault(protocol.ErrorCodeInvalidRequest, "contribution state exceeds the size limit")
	}
	keys := map[string]bool{}
	for _, state := range states {
		var declaration *protocol.PluginUiContribution
		for index := range mount.declarations {
			if mount.declarations[index].ID == state.ID {
				declaration = &mount.declarations[index]
				break
			}
		}
		if declaration == nil {
			return contributionFault(protocol.ErrorCodeInvalidRequest, "contribution is not declared")
		}
		if declaration.Slot == "status" {
			return contributionFault(protocol.ErrorCodePermissionDenied, "status state is owned by the execution Node")
		}
		key := contributionKey(state)
		if keys[key] || !state.Valid(*declaration) {
			return contributionFault(protocol.ErrorCodeInvalidRequest, "invalid contribution state")
		}
		keys[key] = true
	}
	registry.mu.Lock()
	if !registry.owns(mount) {
		registry.mu.Unlock()
		return contributionFault(protocol.ErrorCodeRevisionConflict, "contribution context changed")
	}
	for _, state := range states {
		if state.Scope != nil && !slices.Contains(mount.scopes, *state.Scope) {
			registry.mu.Unlock()
			return contributionFault(protocol.ErrorCodeRevisionConflict, "provider context changed")
		}
		if state.ReplyTo != nil && (*state.ReplyTo == 0 || *state.ReplyTo > mount.interactions[state.ID].sequence) {
			registry.mu.Unlock()
			return contributionFault(protocol.ErrorCodeInvalidRequest, "unknown contribution interaction")
		}
	}
	for _, state := range states {
		if interaction, exists := mount.interactions[state.ID]; exists {
			if state.ReplyTo != nil && *state.ReplyTo < interaction.sequence {
				continue
			}
			if state.ReplyTo != nil && *state.ReplyTo == interaction.sequence {
				interaction.busy = false
				mount.interactions[state.ID] = interaction
			}
			state.Enabled = state.Enabled && !interaction.busy
		}
		key := contributionKey(state)
		mount.states[key], mount.published[key], mount.retained[key] = state, true, true
	}
	registry.mu.Unlock()
	registry.changed()
	return nil
}

func (registry *Registry) dispatch(key ContributionKey, kind protocol.PluginUiEventKind, body json.RawMessage, intent, status bool) error {
	if len(body) == 0 {
		body = json.RawMessage("null")
	}
	var value any
	decoder := json.NewDecoder(bytes.NewReader(body))
	decoder.UseNumber()
	if !json.Valid(body) || decoder.Decode(&value) != nil {
		return contributionFault(protocol.ErrorCodeInvalidRequest, "invalid contribution interaction")
	}
	body, err := json.Marshal(value)
	if err != nil {
		return contributionFault(protocol.ErrorCodeInvalidRequest, "invalid contribution interaction")
	}
	registry.mu.Lock()
	mount, err := registry.current(key.Package)
	if err != nil || !registry.matches(key, mount) || !mount.active {
		registry.mu.Unlock()
		return contributionFault(protocol.ErrorCodeRevisionConflict, "contribution context changed")
	}
	var declaration *protocol.PluginUiContribution
	for index := range mount.declarations {
		if mount.declarations[index].ID == key.ID {
			declaration = &mount.declarations[index]
			break
		}
	}
	if declaration == nil || declaration.Handler == nil {
		registry.mu.Unlock()
		return contributionFault(protocol.ErrorCodeInvalidRequest, "contribution has no action")
	}
	if (declaration.Slot == "status") != status {
		registry.mu.Unlock()
		return contributionFault(protocol.ErrorCodePermissionDenied, "status actions require execution Node state")
	}
	valid := false
	if status {
		switch declaration.Kind {
		case "button", "popover":
			valid = kind == "invoke" && value == nil
		}
		if declaration.Kind == "popover" && kind == "change" {
			_, valid = value.(bool)
		}
	} else {
		state, exists := mount.states[key.ID]
		if !exists || !mount.published[key.ID] || !(state.Visible || intent && declaration.Intent != nil) || !state.Enabled || state.Loading {
			registry.mu.Unlock()
			return contributionFault(protocol.ErrorCodeNotConfigured, "contribution is unavailable")
		}
		switch declaration.Kind {
		case "button", "popover":
			if kind == "invoke" {
				valid = value == nil || intent && declaration.Intent != nil && len(body) <= protocol.PluginUiMaxStateBytes
			}
		}
		if kind == "change" {
			switch declaration.Kind {
			case "toggle", "popover", "indicator":
				_, valid = value.(bool)
			case "select", "menu", "picker":
				choices := declaration.Choices
				if state.Choices != nil {
					choices = *state.Choices
				}
				if selected, ok := value.(string); ok {
					valid = selectableContributionChoice(choices, selected)
				}
			}
		}
		if declaration.Kind == "picker" && kind == "search" {
			query, ok := value.(string)
			valid = ok && len(query) <= 4096
		}
	}
	if !valid {
		registry.mu.Unlock()
		return contributionFault(protocol.ErrorCodeInvalidRequest, "invalid contribution interaction")
	}
	if mount.sequence == math.MaxUint64 {
		registry.mu.Unlock()
		return contributionFault(protocol.ErrorCodeUnavailable, "contribution sequence is exhausted")
	}
	sequence := mount.sequence + 1
	event := protocol.PluginUiEvent{Sequence: sequence, ID: declaration.ID, Handler: *declaration.Handler, Kind: kind, Value: body}
	select {
	case mount.actions <- event:
	default:
		registry.mu.Unlock()
		return contributionFault(protocol.ErrorCodeBusy, "contribution is busy")
	}
	mount.sequence = sequence
	if !status {
		busy := kind != "search"
		mount.interactions[key.ID] = contributionInteraction{sequence: sequence, busy: busy}
		if busy {
			state := mount.states[key.ID]
			state.Enabled = false
			mount.states[key.ID] = state
		}
	}
	registry.mu.Unlock()
	registry.changed()
	return nil
}

func selectableContributionChoice(choices []protocol.PluginUiChoice, selected string) bool {
	for _, choice := range choices {
		if len(choice.Children) == 0 {
			if choice.ID == selected && choice.Enabled {
				return true
			}
			continue
		}
		for _, child := range choice.Children {
			if child.ID == selected && child.Enabled {
				return true
			}
		}
	}
	return false
}

func (registry *Registry) closePopover(mount *contributionMount, id string) error {
	declared := slices.ContainsFunc(mount.declarations, func(declaration protocol.PluginUiContribution) bool {
		return declaration.ID == id && declaration.Slot == "status" && declaration.Kind == "popover"
	})
	if !declared {
		return contributionFault(protocol.ErrorCodeInvalidRequest, "status popover is not declared")
	}
	registry.mu.Lock()
	if !registry.owns(mount) {
		registry.mu.Unlock()
		return contributionFault(protocol.ErrorCodeRevisionConflict, "contribution context changed")
	}
	mount.closed[id] = true
	registry.mu.Unlock()
	registry.changed()
	return nil
}

func (registry *Registry) next(ctx context.Context, mount *contributionMount) (json.RawMessage, error) {
	select {
	case <-ctx.Done():
		return nil, ctx.Err()
	case <-mount.done:
		return nil, contributionFault(protocol.ErrorCodeUnavailable, "contribution runtime is closed")
	case mount.receiver <- struct{}{}:
	}
	defer func() { <-mount.receiver }()
	select {
	case <-ctx.Done():
		return nil, ctx.Err()
	case <-mount.done:
		return nil, contributionFault(protocol.ErrorCodeUnavailable, "contribution runtime is closed")
	case event := <-mount.actions:
		registry.mu.Lock()
		current := registry.owns(mount)
		registry.mu.Unlock()
		if !current {
			return nil, contributionFault(protocol.ErrorCodeRevisionConflict, "contribution context changed")
		}
		encoded, err := json.Marshal(event)
		return json.RawMessage(encoded), err
	}
}
