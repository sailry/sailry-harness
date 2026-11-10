package plugins

import (
	"context"
	"encoding/json"
	"fmt"
	"strings"
)

func (binding *NativeBinding) uiOperation(ctx context.Context, name string, arguments []json.RawMessage) (bool, json.RawMessage, error) {
	if operation := binding.Operations["sailry/ui."+name]; operation != nil {
		value, err := operation(ctx, arguments)
		return true, value, err
	}
	queues := map[string]string{
		"nextControlEvent": "control", "nextNavigationTabEvent": "navigation-tabs",
		"nextTableEvent": "table", "nextTreeEvent": "tree", "nextMenuEvent": "menu",
		"nextContextMenuEvent": "context-menu", "modal_closed": "modal",
		"nextPickerEvent": "picker", "nextToastEvent": "toast", "nextCardEvent": "card",
	}
	if queue, exists := queues[name]; exists {
		value, err := binding.next(ctx, queue)
		return true, value, err
	}
	switch name {
	case "focusEntry":
		value, err := hostOptional[*string](arguments, 0, nil)
		if err != nil {
			return true, nil, err
		}
		binding.state.mu.Lock()
		binding.state.focus = ""
		if value != nil {
			binding.state.focus = *value
		}
		binding.state.mu.Unlock()
		if binding.Invalidate != nil {
			binding.Invalidate()
		}
		return true, json.RawMessage("null"), nil
	case "collapseOpen":
		identity, err := hostValue[string](arguments, 0)
		if err != nil {
			return true, nil, err
		}
		binding.state.mu.Lock()
		defer binding.state.mu.Unlock()
		if value := binding.state.collapses[identity]; value != nil {
			body, err := hostJSON(*value)
			return true, body, err
		}
		return true, json.RawMessage("null"), nil
	case "registerShortcuts":
		values, err := hostValue[[]nativeShortcut](arguments, 0)
		if err != nil {
			return true, nil, err
		}
		if len(values) > 32 {
			return true, nil, hostInvalid("plugin shortcut limit exceeded")
		}
		for _, shortcut := range values {
			if strings.TrimSpace(shortcut.Action) == "" || len(shortcut.Action) > 128 {
				return true, nil, hostInvalid("invalid plugin shortcut")
			}
			if shortcut.Context != nil && (strings.TrimSpace(*shortcut.Context) == "" || len(*shortcut.Context) > 256) {
				return true, nil, hostInvalid("invalid plugin shortcut context")
			}
			if shortcut.Context != nil {
				if _, err := parseNativeContextPredicate(*shortcut.Context); err != nil {
					return true, nil, err
				}
			}
			if _, err := nativeKeystrokes(shortcut.Keystroke); err != nil {
				return true, nil, err
			}
		}
		binding.state.mu.Lock()
		binding.state.shortcuts = values
		binding.state.shortcutVersion++
		if binding.state.shortcut == "" {
			binding.state.shortcut = fmt.Sprintf("SailryPlugin%d", binding.state.sequence)
		}
		scope := binding.state.shortcut
		binding.state.mu.Unlock()
		body, err := hostJSON(scope)
		return true, body, err
	}
	return false, nil, nil
}

func (binding *NativeBinding) HeaderSnapshot() json.RawMessage {
	binding.state.mu.Lock()
	defer binding.state.mu.Unlock()
	return append(json.RawMessage{}, binding.state.header...)
}

func (binding *NativeBinding) invoke(module, name string, arguments ...any) {
	operation := binding.Operations[module+"."+name]
	if operation == nil {
		binding.fail(hostUnavailable(module + "." + name + " is unavailable in this view"))
		return
	}
	input := make([]json.RawMessage, len(arguments))
	for index, value := range arguments {
		body, err := hostJSON(value)
		if err != nil {
			binding.fail(err)
			return
		}
		input[index] = body
	}
	go func() {
		_, err := operation(binding.state.ctx, input)
		if err != nil {
			binding.fail(err)
		}
	}()
}

func (binding *NativeBinding) fail(err error) {
	binding.state.mu.Lock()
	binding.state.failure = err
	binding.state.mu.Unlock()
	if binding.Invalidate != nil {
		binding.Invalidate()
	}
}
