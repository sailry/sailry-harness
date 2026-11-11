package shell

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
)

// Shortcut is a key of the window: its id, label and default key, as the
// Shortcuts settings list them.
type Shortcut = settings.Shortcut

// Shortcuts are the reference's window keys, ⌘ standing for Ctrl off
// macOS: the Shortcuts settings' registry, which the user may rebind
// (App.binding), then Settings, Find and the conversation's own keys, which
// are fixed: Stop, Focus the composer and the model controls act on the
// conversation page only, and the reference lists them in neither the
// Shortcuts settings nor the menu. The system's shortcuts are not the
// window's. The panel's resource keys follow (panel.Resources).
var Shortcuts = append(window(settings.Registry),
	Shortcut{ID: "app.settings", Label: "settings", Mods: ui.Cmd, Key: ui.KeyComma},
	Shortcut{ID: "conversation.find", Label: "chat_search", Mods: ui.Cmd, Key: ui.KeyF},
	Shortcut{ID: "conversation.stop", Label: "chat_stop", Mods: ui.Cmd, Key: ui.KeyPeriod},
	Shortcut{ID: "composer.focus", Label: "chat_message_placeholder", Mods: ui.Cmd, Key: ui.KeyL},
	Shortcut{ID: "composer.models", Label: "composer_model_settings", Mods: ui.Cmd | ui.Shift, Key: ui.KeyM},
)

func window(all []Shortcut) []Shortcut {
	var out []Shortcut
	for _, s := range all {
		if !s.System {
			out = append(out, s)
		}
	}
	return out
}

// CloseFocused is ⌘W. The reference binds it on the panes that close, not on
// the window: the right panel and an independent page take it while the focus
// is inside them (App.pane), and anywhere else it does nothing.
var CloseFocused = Shortcut{ID: "close", Label: "menu_close_tab", Mods: ui.Cmd, Key: ui.KeyW}

// binding is a shortcut on the key the user gave it; false when turned
// off.
func (a *App) binding(s Shortcut) (Shortcut, bool) { return a.Settings.Keys.Bound(s) }

// hint is the text of a shortcut by its id, on its key now.
func (a *App) hint(id string) string {
	for _, s := range Shortcuts {
		if s.ID != id {
			continue
		}
		if s, ok := a.binding(s); ok {
			return kit.KeyText(s.Mods, s.Key)
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
//
// The native menu takes its items' keys before the window does (menu.go),
// so their commands come back through Do and run here in the next frame,
// scoped as the key would have been: none while a modal dialog of the
// window shows (dialog), then Close Tab by where the focus is
// (closeFocused). This is the scoping MyGo's menus lack.

// shortcuts runs the window's shortcuts and the commands the menu sent.
// While the Shortcuts page records a key, the window takes none, so every
// key reaches the recorder, and a menu item's key is recorded instead of
// run (the menu takes its keys before the window).
func (a *App) shortcuts(c *ui.Context) {
	sent := a.sent
	a.sent = nil
	a.keyed = ""
	a.modal = kit.TakeModal(c)
	kit.SweepMenus(c)
	a.syncMenuKeys()
	if a.Settings.Keys.Recording() && (a.Page != SettingsPage || a.Settings.Section != settings.Shortcuts) {
		// The recorder is not in the window: however the page was left, the
		// recording ends with it rather than taking the window's keys.
		a.Settings.Keys.Cancel()
	}
	if a.Settings.Keys.Recording() {
		for _, id := range sent {
			if s, ok := shortcutOf(id); ok {
				a.Settings.Keys.Take(settings.Chord{Mods: s.Mods, Key: s.Key})
			}
		}
		return
	}
	for _, s := range Shortcuts {
		s, ok := a.binding(s)
		if !ok || !c.Shortcut(s.Mods, s.Key) || c.ShortcutRepeated() && s.ID != "conversation.previous" && s.ID != "conversation.next" {
			continue
		}
		a.run(c, s.ID)
	}
	for _, id := range sent {
		switch {
		case a.dialog():
		case id == CloseFocused.ID:
			a.closeFocused(c)
		default:
			a.run(c, id)
		}
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

// Do sends a command of the native menu, a Shortcut's ID or another of
// the menu's (menu.go), to run in the next frame. Call it on the main
// thread, where menu items' Click runs, then redraw the window.
func (a *App) Do(id string) {
	a.sent = append(a.sent, id)
}

// dialog reports whether one of the window's dialogs shows, which holds
// the window's commands as Element.Modal holds its shortcuts: one this
// file opens, known the frame it opens in, or any modal dialog Kit built
// in the last frame (a.modal), wherever in the window it was opened.
func (a *App) dialog() bool {
	return a.modal || a.search.open || a.reading != nil && a.reading.open || a.Sidebar.Editing() || a.Welcome.Open || a.selected.open
}

// closeFocused is Close Tab of the menu: ⌘W of the pane the focus is in
// (App.pane), nothing outside them.
func (a *App) closeFocused(c *ui.Context) {
	switch {
	case a.PanelOpen && c.FocusWithin(a.panelPane):
		a.closeTab(c)
	case c.FocusWithin(a.mainPane):
		a.ClosePage()
	}
}

// run carries out a window command.
func (a *App) run(c *ui.Context, id string) {
	if _, exists := a.pluginFeatures[id]; exists {
		a.openFeature(c, id)
		return
	}
	if p, ok := pageFeatures[id]; ok {
		a.Navigate(p)
		return
	}
	switch id {
	case "file.save":
		if project := a.project(); project != nil {
			project.SaveFocused(c)
		}
	case "help.logs":
		if a.Hooks.OpenLogs != nil {
			a.Hooks.OpenLogs()
		}
	case "conversation.find":
		// The native menu takes ⌘F first: in a terminal tab it finds in the
		// terminal.
		if a.PanelOpen && c.FocusWithin(a.panelPane) && a.Panel.Find() {
			return
		}
		// Find in the conversation: from the conversation, its input or the search
		// bar, not from the panel. In a split it is the focused pane's
		// conversation, which has its own bar; a terminal pane has none.
		if a.Page == ConversationPage && !(a.PanelOpen && c.FocusWithin(a.panelPane)) {
			if target := a.findTarget(); target != "" {
				a.Messages.OpenSearch(target)
			}
		}
	case "conversation.stop", "composer.focus", "composer.models":
		// The conversation's own keys: its page carries them out with its
		// composer this frame (App.conversation), from anywhere in the
		// page but the panel.
		if a.Page == ConversationPage && !(a.PanelOpen && c.FocusWithin(a.panelPane)) {
			a.keyed = id
		}
	case "conversation.new":
		a.Page = ConversationPage
		a.Sidebar.NewChat()
	case "project.new":
		a.Sidebar.OpenEditor(nil)
	case "folder.open":
		a.Sidebar.OpenFolder(c, a.Host)
	case "host.refresh":
		if a.Hooks.Refresh != nil {
			a.Hooks.Refresh()
		} else {
			kit.PreviewNotice(c)
		}
	case "app.search":
		if a.search.open {
			a.search.open = false
		} else {
			a.openSearch()
		}
	case "sidebar.toggle":
		a.toggleNav()
	case "panel.toggle":
		a.togglePanel()
	case "conversation.previous", "conversation.next":
		if next := adjacent(a, id == "conversation.next"); next != "" {
			a.Page = ConversationPage
			a.Sidebar.Selected = next
		}
	case "app.shortcuts":
		a.Page = SettingsPage
		a.Settings.Open(settings.Shortcuts)
	case "app.about":
		a.Page = SettingsPage
		a.Settings.Open(settings.About)
	case "app.settings":
		a.Navigate(SettingsPage)
	case "help.website":
		c.OpenURL(Website)
	case "help.feedback":
		c.OpenURL(Feedback)
	case "app.welcome":
		a.Welcome.Open = true
	case "system.activate":
		// The system's activate shortcut: the window comes forward
		// with the search.
		a.openSearch()
	case "system.selection":
		a.captureSelection()
	}
}
