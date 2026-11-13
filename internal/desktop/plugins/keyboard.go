package plugins

import (
	"encoding/json"
	"strings"

	"github.com/MoeForge/moejs"
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
)

type nativeKeyCallbacks struct {
	down, up CallbackRef
}

func (mounted *Mounted) keyInput(handlers []nativeKeyCallbacks) func(ui.InputEvent) bool {
	if len(handlers) == 0 || mounted.suspended || mounted.closed {
		return nil
	}
	return func(event ui.InputEvent) bool {
		if mounted.suspended || mounted.closed || event.Kind != ui.InputKeyDown && event.Kind != ui.InputKeyUp {
			return false
		}
		data, _ := json.Marshal(map[string]any{
			"key": nativeKeyName(event.Key), "is_held": event.Repeat,
			"modifiers": map[string]bool{"platform": event.Mods&ui.Super != 0, "control": event.Mods&ui.Ctrl != 0,
				"alt": event.Mods&ui.Alt != 0, "shift": event.Mods&ui.Shift != 0},
		})
		// MyGo HandleInput belongs to the focused control. Script key events
		// bubble through this captured chain, innermost first, so a native
		// field can take Escape without replacing the field's ordinary input.
		for index := len(handlers) - 1; index >= 0; index-- {
			callback := handlers[index].down
			if event.Kind == ui.InputKeyUp {
				callback = handlers[index].up
			}
			if callback.ID == 0 {
				continue
			}
			stopped, err := mounted.dispatch("dispatchInput", callback, data)
			if err != nil {
				mounted.failure = err
				return true
			}
			if stopped == true {
				return true
			}
		}
		return false
	}
}

func nativeKeyName(key ui.Key) string {
	switch key {
	case ui.KeyEscape:
		return "escape"
	case ui.KeyLeft:
		return "left"
	case ui.KeyRight:
		return "right"
	case ui.KeyUp:
		return "up"
	case ui.KeyDown:
		return "down"
	case ui.KeyUnknown:
		return ""
	}
	return strings.ToLower(kit.KeyName(key))
}

func (mounted *Mounted) focusHandle(id uint64) *ui.Handle {
	handle := mounted.focusHandles[id]
	if handle == nil {
		handle = &ui.Handle{}
		mounted.focusHandles[id] = handle
	}
	return handle
}

func (mounted *Mounted) nativeFocus(_ *moejs.Realm, _ moejs.Value, arguments []moejs.Value) (moejs.Value, error) {
	id := runtimeUint(arguments, 1)
	switch arguments[0].String() {
	case "focus":
		for other, handle := range mounted.focusHandles {
			if other != id {
				handle.CancelFocus()
			}
		}
		mounted.focusHandle(id).Focus()
	case "focused":
		handle := mounted.focusHandles[id]
		return mounted.engine.FromGo(handle != nil && mounted.focusContext != nil && handle.Focused(mounted.focusContext))
	case "release":
		if handle := mounted.focusHandles[id]; handle != nil {
			handle.CancelFocus()
			delete(mounted.focusHandles, id)
		}
	}
	return moejs.Undefined(), nil
}
