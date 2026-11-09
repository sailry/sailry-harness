package shell

import (
	"runtime"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/i18n"
	"github.com/sailry/sailry-go/internal/panel"
)

// Shortcut is a key of the window (shortcuts.rs, shell/shortcuts.rs):
// its id and label, as the Shortcuts settings will list them.
type Shortcut struct {
	ID    string
	Label string // i18n key
	Mods  ui.Modifiers
	Key   ui.Key
}

// Shortcuts are the reference's keys this phase handles, ⌘ standing for
// Ctrl off macOS. The panel's resource keys follow (panel.Resources).
var Shortcuts = []Shortcut{
	{"conversation.new", "menu_new_conversation", ui.Cmd, ui.KeyN},
	{"project.new", "menu_new_project", ui.Cmd | ui.Alt, ui.KeyN},
	{"folder.open", "menu_open_folder", ui.Cmd, ui.KeyO},
	{"app.search", "search", ui.Cmd, ui.KeyK},
	{"sidebar.toggle", "sidebar_toggle", ui.Cmd, ui.KeyB},
	{"panel.toggle", "shortcut_inspector", ui.Cmd | ui.Alt, ui.KeyB},
	{"host.refresh", "shortcut_sync", ui.Cmd | ui.Shift, ui.KeyR},
	{"conversation.previous", "shortcut_previous_conversation", ui.Cmd | ui.Shift, ui.KeyBracketLeft},
	{"conversation.next", "shortcut_next_conversation", ui.Cmd | ui.Shift, ui.KeyBracketRight},
	{"app.shortcuts", "settings_shortcuts", ui.Cmd, ui.KeySlash},
	{"app.settings", "settings", ui.Cmd, ui.KeyComma},
}

// CloseFocused is ⌘W (shell/shortcuts.rs CloseFocused). The reference binds
// it on the panes that close, not on the window: the right panel and an
// independent page take it while the focus is inside them (App.pane), and
// anywhere else it does nothing.
var CloseFocused = Shortcut{"close", "menu_close_tab", ui.Cmd, ui.KeyW}

// KeyText writes a shortcut as menus show it: ⌥⇧⌘B on macOS, Ctrl+Alt+B
// elsewhere.
func KeyText(mods ui.Modifiers, key ui.Key) string {
	mac := runtime.GOOS == "darwin"
	name := keyName(key)
	if mac {
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

func keyName(k ui.Key) string {
	switch k {
	case ui.KeyBracketLeft:
		return "["
	case ui.KeyBracketRight:
		return "]"
	case ui.KeySlash:
		return "/"
	case ui.KeyComma:
		return ","
	case ui.KeyBackquote:
		return "`"
	}
	if k >= ui.KeyA && k <= ui.KeyZ {
		return string(rune('A' + int(k-ui.KeyA)))
	}
	return "?"
}

// hint is the text of a shortcut by its id.
func hint(id string) string {
	for _, s := range Shortcuts {
		if s.ID == id {
			return KeyText(s.Mods, s.Key)
		}
	}
	return ""
}

// Keys go to the innermost scope that takes them, as MyGo dispatches a key:
// the focused element and those around it first (a text input's editing
// keys, then a pane's Element.Shortcut: App.pane), then the overlays,
// the one on top first (Element.OverlayShortcut: PopoverBase and
// DialogBase close on Escape), then the window (Context.Shortcut, below),
// which a modal dialog holds. A key taken by one never reaches the next.

// shortcuts runs the window's shortcuts.
func (a *App) shortcuts(c *ui.Context) {
	for _, s := range Shortcuts {
		if !c.Shortcut(s.Mods, s.Key) || c.ShortcutRepeated() && s.ID != "conversation.previous" && s.ID != "conversation.next" {
			continue
		}
		a.run(c, s.ID)
	}
	if a.Page != ConversationPage {
		return
	}
	for _, r := range panel.Resources {
		if r.Key != 0 && c.Shortcut(r.Mods, r.Key) && !c.ShortcutRepeated() {
			a.openPanel(r.Kind)
		}
	}
}

// run carries out a window command.
func (a *App) run(c *ui.Context, id string) {
	switch id {
	case "conversation.new":
		a.Page = ConversationPage
		a.Sidebar.NewChat()
	case "project.new":
		a.Sidebar.OpenEditor(nil)
	case "folder.open", "host.refresh":
		// Opening a folder reads the disk; a sync asks the Node.
		c.Toast(i18n.T(c, "preview_notice"))
	case "app.search":
		a.search.open = !a.search.open
	case "sidebar.toggle":
		a.NavOpen = !a.NavOpen
	case "panel.toggle":
		a.togglePanel()
	case "conversation.previous", "conversation.next":
		if next := adjacent(a, id == "conversation.next"); next != "" {
			a.Page = ConversationPage
			a.Sidebar.Selected = next
		}
	case "app.shortcuts", "app.settings":
		// Settings has no Shortcuts section before phase 5.
		a.Page = SettingsPage
	}
}
