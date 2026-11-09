package settings

import (
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
)

// Overrides uses the controller's portable GPUI keystroke notation.
func (k *Keys) Overrides() map[string]string {
	out := map[string]string{}
	for id, ch := range k.set {
		out[id] = formatChord(ch)
	}
	return out
}

func (k *Keys) SetOverrides(values map[string]string) {
	k.set = map[string]Chord{}
	for id, value := range values {
		if _, ok := find(id); !ok {
			continue
		}
		if value == "" {
			k.set[id] = Chord{}
			continue
		}
		if ch, ok := parseChord(value); ok {
			k.set[id] = ch
		}
	}
}

var portableKeys = map[ui.Key]string{
	ui.KeySpace: "space", ui.KeyEnter: "enter", ui.KeyTab: "tab", ui.KeyBackspace: "backspace", ui.KeyDelete: "delete",
	ui.KeyHome: "home", ui.KeyEnd: "end", ui.KeyPageUp: "pageup", ui.KeyPageDown: "pagedown", ui.KeyInsert: "insert",
	ui.KeyLeft: "left", ui.KeyRight: "right", ui.KeyUp: "up", ui.KeyDown: "down",
}

func formatChord(ch Chord) string {
	if ch.Key == 0 {
		return ""
	}
	var parts []string
	mods := ch.Mods
	for _, entry := range []struct {
		mod  ui.Modifiers
		name string
	}{{ui.Cmd, "secondary"}, {ui.Ctrl, "ctrl"}, {ui.Alt, "alt"}, {ui.Shift, "shift"}, {ui.Super, "super"}} {
		if mods&entry.mod != 0 {
			parts = append(parts, entry.name)
			mods &^= entry.mod
		}
	}
	name := strings.ToLower(kit.KeyName(ch.Key))
	if value, ok := portableKeys[ch.Key]; ok {
		name = value
	}
	return strings.Join(append(parts, name), "-")
}

func parseChord(value string) (Chord, bool) {
	var ch Chord
	for {
		found := false
		for _, entry := range []struct {
			name string
			mod  ui.Modifiers
		}{{"secondary-", ui.Cmd}, {"cmd-", ui.Super}, {"super-", ui.Super}, {"ctrl-", ui.Ctrl}, {"alt-", ui.Alt}, {"shift-", ui.Shift}} {
			if strings.HasPrefix(value, entry.name) {
				ch.Mods |= entry.mod
				value = strings.TrimPrefix(value, entry.name)
				found = true
				break
			}
		}
		if !found {
			break
		}
	}
	for key, name := range portableKeys {
		if value == name {
			ch.Key = key
			return ch, true
		}
	}
	for key := ui.KeyA; key <= ui.KeyZ; key++ {
		if strings.EqualFold(kit.KeyName(key), value) {
			ch.Key = key
			return ch, true
		}
	}
	for key := ui.Key0; key <= ui.Key9; key++ {
		if kit.KeyName(key) == value {
			ch.Key = key
			return ch, true
		}
	}
	for _, key := range []ui.Key{ui.KeyBracketLeft, ui.KeyBracketRight, ui.KeySlash, ui.KeyComma, ui.KeyBackquote, ui.KeyPeriod, ui.KeyMinus, ui.KeyEqual, ui.KeySemicolon, ui.KeyQuote, ui.KeyBackslash, ui.KeyF1, ui.KeyF2, ui.KeyF3, ui.KeyF4, ui.KeyF5, ui.KeyF6, ui.KeyF7, ui.KeyF8, ui.KeyF9, ui.KeyF10, ui.KeyF11, ui.KeyF12} {
		if strings.EqualFold(kit.KeyName(key), value) {
			ch.Key = key
			return ch, true
		}
	}
	return ch, false
}
