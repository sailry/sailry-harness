package plugins

import (
	"encoding/json"
	"runtime"
	"strings"

	"github.com/egoist/mygo/ui"
)

type nativeShortcut struct {
	Keystroke string  `json:"keystroke"`
	Action    string  `json:"action"`
	Context   *string `json:"context"`
}

type nativeShortcutStroke struct {
	modifiers ui.Modifiers
	key       ui.Key
}

func nativeKeystrokes(value string) ([]nativeShortcutStroke, error) {
	parts := strings.Fields(value)
	if len(parts) == 0 || len(parts) > 2 {
		return nil, hostInvalid("invalid plugin shortcut")
	}
	strokes := make([]nativeShortcutStroke, 0, len(parts))
	for _, part := range parts {
		modifiers, key, err := nativeKeystroke(part)
		if err != nil {
			return nil, err
		}
		strokes = append(strokes, nativeShortcutStroke{modifiers: modifiers, key: key})
	}
	return strokes, nil
}

func nativeKeystroke(value string) (ui.Modifiers, ui.Key, error) {
	if value == "-" {
		value = "minus"
	} else if strings.HasSuffix(value, "--") {
		value = strings.TrimSuffix(value, "-") + "minus"
	}
	parts := strings.Split(strings.ToLower(value), "-")
	var modifiers ui.Modifiers
	if len(parts) == 0 {
		return 0, 0, hostInvalid("invalid plugin shortcut")
	}
	for _, part := range parts[:len(parts)-1] {
		switch part {
		case "secondary", "cmd", "command":
			if runtime.GOOS == "darwin" {
				modifiers |= ui.Super
			} else {
				modifiers |= ui.Ctrl
			}
		case "ctrl", "control":
			modifiers |= ui.Ctrl
		case "alt", "option":
			modifiers |= ui.Alt
		case "shift":
			modifiers |= ui.Shift
		case "super":
			modifiers |= ui.Super
		default:
			return 0, 0, hostInvalid("invalid plugin shortcut")
		}
	}
	keyName := parts[len(parts)-1]
	keys := map[string]ui.Key{
		"enter": ui.KeyEnter, "escape": ui.KeyEscape, "esc": ui.KeyEscape, "space": ui.KeySpace,
		"tab": ui.KeyTab, "backspace": ui.KeyBackspace, "delete": ui.KeyDelete,
		"up": ui.KeyUp, "down": ui.KeyDown, "left": ui.KeyLeft, "right": ui.KeyRight,
		"home": ui.KeyHome, "end": ui.KeyEnd, "pageup": ui.KeyPageUp, "pagedown": ui.KeyPageDown,
		"[": ui.KeyBracketLeft, "]": ui.KeyBracketRight, "/": ui.KeySlash, "\\": ui.KeyBackslash,
		"`": ui.KeyBackquote, ";": ui.KeySemicolon, "'": ui.KeyQuote, ",": ui.KeyComma, ".": ui.KeyPeriod,
		"=": ui.KeyEqual, "minus": ui.KeyMinus, "f1": ui.KeyF1, "f2": ui.KeyF2, "f3": ui.KeyF3, "f4": ui.KeyF4,
		"f5": ui.KeyF5, "f6": ui.KeyF6, "f7": ui.KeyF7, "f8": ui.KeyF8,
		"f9": ui.KeyF9, "f10": ui.KeyF10, "f11": ui.KeyF11, "f12": ui.KeyF12,
	}
	if key, exists := keys[keyName]; exists {
		return modifiers, key, nil
	}
	if len(keyName) == 1 {
		if character := keyName[0]; character >= 'a' && character <= 'z' {
			return modifiers, ui.KeyA + ui.Key(character-'a'), nil
		}
		if character := keyName[0]; character >= '0' && character <= '9' {
			return modifiers, ui.Key0 + ui.Key(character-'0'), nil
		}
	}
	return 0, 0, hostInvalid("invalid plugin shortcut")
}

type nativeShortcutProgress struct {
	version   uint64
	positions []int
}

func (binding NativeBinding) BindActions(view *ui.Context, region ui.Element, element NativeElement, emit func(CallbackRef, json.RawMessage)) {
	binding.state.mu.Lock()
	if !binding.state.actionFrame.Equal(view.Now()) {
		clear(binding.state.actionElements)
		binding.state.actionFrame = view.Now()
	}
	binding.state.actionElements[element.ID] = region
	binding.state.mu.Unlock()
	hasAction := false
	for name := range element.Callbacks {
		if strings.HasPrefix(name, "on_action:") {
			hasAction = true
			break
		}
	}
	if !hasAction {
		return
	}
	binding.state.mu.Lock()
	shortcuts := append([]nativeShortcut{}, binding.state.shortcuts...)
	version, scope := binding.state.shortcutVersion, binding.state.shortcut
	binding.state.mu.Unlock()
	progress := ui.Local(region, "plugin-shortcuts", func() nativeShortcutProgress { return nativeShortcutProgress{} })
	if progress.version != version {
		progress.version, progress.positions = version, make([]int, len(shortcuts))
	}
	if len(progress.positions) != len(shortcuts) {
		progress.positions = make([]int, len(shortcuts))
	}
	positions := progress.positions
	if !region.FocusWithin() {
		clear(positions)
		return
	}
	contexts, err := binding.focusedContexts(&element)
	if err != nil {
		binding.fail(err)
		return
	}
	pressed := map[nativeShortcutStroke]bool{}
	sequences := make([][]nativeShortcutStroke, len(shortcuts))
	depths := make([]int, len(shortcuts))
	for index, shortcut := range shortcuts {
		if _, declared := element.Callbacks["on_action:"+shortcut.Action]; !declared {
			continue
		}
		predicate := &nativeContextPredicate{name: scope}
		if shortcut.Context != nil {
			relative, err := parseNativeContextPredicate(*shortcut.Context)
			if err != nil {
				binding.fail(err)
				return
			}
			predicate = &nativeContextPredicate{operator: ">", left: predicate, right: relative}
		}
		depths[index] = predicate.depth(contexts)
		if depths[index] < 0 {
			positions[index] = 0
			continue
		}
		strokes, err := nativeKeystrokes(shortcut.Keystroke)
		if err != nil {
			continue
		}
		sequences[index] = strokes
		if positions[index] >= len(strokes) {
			positions[index] = 0
		}
		stroke := strokes[positions[index]]
		if _, registered := pressed[stroke]; !registered {
			pressed[stroke] = region.Shortcut(stroke.modifiers, stroke.key)
		}
	}
	region.ObserveInput(func(event ui.InputEvent) {
		if event.Kind != ui.InputKeyDown {
			return
		}
		for index, strokes := range sequences {
			if len(strokes) > 0 {
				expected := strokes[positions[index]]
				if expected.modifiers == event.Mods && expected.key == event.Key {
					return
				}
			}
		}
		clear(positions)
	})
	changed, completed := false, -1
	for index, strokes := range sequences {
		if len(strokes) == 0 || !pressed[strokes[positions[index]]] {
			continue
		}
		changed = true
		positions[index]++
		if positions[index] == len(strokes) {
			positions[index] = 0
			if completed < 0 || depths[index] >= depths[completed] {
				completed = index
			}
		}
	}
	if completed >= 0 {
		clear(positions)
		emit(element.Callbacks["on_action:"+shortcuts[completed].Action], json.RawMessage(`{}`))
	}
	if changed {
		for index, strokes := range sequences {
			if len(strokes) != 0 {
				next := strokes[positions[index]]
				if _, registered := pressed[next]; !registered {
					region.Shortcut(next.modifiers, next.key)
					pressed[next] = false
				}
			}
		}
	}
}

func (binding *NativeBinding) focusedContexts(element *NativeElement) ([]nativeKeyContext, error) {
	contexts := []nativeKeyContext{}
	for _, style := range element.Styles {
		if style.Name != "key_context" {
			continue
		}
		value, err := hostValue[string](style.Arguments, 0)
		if err != nil {
			return nil, err
		}
		context, err := parseNativeKeyContext(value)
		if err != nil {
			return nil, err
		}
		contexts = append(contexts, context)
	}
	switch {
	case element.Module == "sailry/forms" && (element.Export == "TextField" || element.Export == "NumberField"):
		contexts = append(contexts, nativeKeyContext{"Input": nil})
	case element.Module == "sailry/credentials" && element.Export == "SecretField":
		contexts = append(contexts, nativeKeyContext{"Input": nil})
	case element.Export == "Input" || element.Export == "Textarea" || element.Export == "NumberInput" || element.Export == "OtpInput":
		contexts = append(contexts, nativeKeyContext{"Input": nil})
	case element.Export == "Editor" || element.Export == "Diff":
		contexts = append(contexts, nativeKeyContext{"Editor": nil})
	}
	children := append([]*NativeElement{}, element.Children...)
	for _, slots := range element.Slots {
		children = append(children, slots...)
	}
	for _, child := range children {
		binding.state.mu.Lock()
		rendered, exists := binding.state.actionElements[child.ID]
		binding.state.mu.Unlock()
		if !exists || !rendered.FocusWithin() {
			continue
		}
		nested, err := binding.focusedContexts(child)
		if err != nil {
			return nil, err
		}
		contexts = append(contexts, nested...)
		break
	}
	return contexts, nil
}
