package kit

import (
	"runtime"
	"strconv"
	"strings"

	"github.com/egoist/mygo/ui"
)

// KeyText writes a shortcut as menus show it: ⌥⇧⌘B on macOS, Ctrl+Alt+B
// elsewhere.
func KeyText(mods ui.Modifiers, key ui.Key) string {
	name := KeyName(key)
	if runtime.GOOS == "darwin" {
		var b strings.Builder
		for _, m := range []struct {
			mod ui.Modifiers
			s   string
		}{{ui.Ctrl, "⌃"}, {ui.Alt, "⌥"}, {ui.Shift, "⇧"}, {ui.Super, "⌘"}} {
			if mods&m.mod != 0 {
				b.WriteString(m.s)
			}
		}
		return b.String() + name
	}
	var parts []string
	for _, m := range []struct {
		mod ui.Modifiers
		s   string
	}{{ui.Ctrl, "Ctrl"}, {ui.Alt, "Alt"}, {ui.Shift, "Shift"}, {ui.Super, "Super"}} {
		if mods&m.mod != 0 {
			parts = append(parts, m.s)
		}
	}
	return strings.Join(append(parts, name), "+")
}

// keyNames are the names of the keys that are not a letter, a digit or a
// function key.
var keyNames = map[ui.Key]string{
	ui.KeyBracketLeft: "[", ui.KeyBracketRight: "]", ui.KeySlash: "/", ui.KeyComma: ",",
	ui.KeyBackquote: "`", ui.KeyPeriod: ".", ui.KeyMinus: "-", ui.KeyEqual: "=",
	ui.KeySemicolon: ";", ui.KeyQuote: "'", ui.KeyBackslash: "\\",
	ui.KeySpace: "Space", ui.KeyEnter: "Enter", ui.KeyTab: "Tab", ui.KeyBackspace: "Backspace",
	ui.KeyDelete: "Delete", ui.KeyHome: "Home", ui.KeyEnd: "End", ui.KeyPageUp: "PageUp",
	ui.KeyPageDown: "PageDown", ui.KeyLeft: "←", ui.KeyRight: "→", ui.KeyUp: "↑", ui.KeyDown: "↓",
	ui.KeyInsert: "Insert",
}

var functionKeys = [...]ui.Key{ui.KeyF1, ui.KeyF2, ui.KeyF3, ui.KeyF4, ui.KeyF5, ui.KeyF6,
	ui.KeyF7, ui.KeyF8, ui.KeyF9, ui.KeyF10, ui.KeyF11, ui.KeyF12}

// KeyName is a key's name in a shortcut: B, /, F5, Space.
func KeyName(k ui.Key) string {
	switch {
	case k >= ui.KeyA && k <= ui.KeyZ:
		return string(rune('A' + int(k-ui.KeyA)))
	case k >= ui.Key0 && k <= ui.Key9:
		return string(rune('0' + int(k-ui.Key0)))
	}
	for i, f := range functionKeys {
		if k == f {
			return "F" + strconv.Itoa(i+1)
		}
	}
	if s, ok := keyNames[k]; ok {
		return s
	}
	return "?"
}
