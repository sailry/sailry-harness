package shell

import (
	"strings"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/i18n"
	"github.com/sailry/sailry-go/internal/kit"
)

// Website and Feedback are the Help menu's pages (app_menu.rs).
const (
	Website  = "https://sailry.dev"
	Feedback = "https://github.com/sailry/sailry-harness/issues"
)

// Menu is the native app menu in lang (app_menu.rs menus): App, File,
// Edit, View, Window and Help, with the reference's items and keys, then
// the rail's features in View. The app's items send their command's ID
// through send, which cmd/sailry passes to Do: the menu takes their keys
// before the window does. The system's items are MyGo's roles under the
// reference's labels and keys: app_menu.rs binds Close Window, Hide, Hide
// Others, Minimize and Full Screen on cmd (ui.Super), the rest on
// secondary (ui.Cmd). Save, Find in Conversation and Logs stay disabled
// while the preview has no file editor, find bar or log folder.
func (a *App) Menu(lang string, send func(id string)) []*mygo.MenuItem {
	t := func(key string) string { return i18n.In(lang, key) }
	item := func(label, id string) *mygo.MenuItem {
		it := &mygo.MenuItem{ID: id, Label: t(label), Click: func(*mygo.MenuItem, *mygo.Window) { send(id) }}
		if s, ok := shortcutOf(id); ok {
			it.Accelerator = Accelerator(s.Mods, s.Key)
		}
		return it
	}
	role := func(label string, r mygo.MenuRole, mods ui.Modifiers, key ui.Key) *mygo.MenuItem {
		it := &mygo.MenuItem{Label: t(label), Role: r}
		if key != 0 {
			it.Accelerator = Accelerator(mods, key)
		}
		return it
	}
	off := func(label, id string, mods ui.Modifiers, key ui.Key) *mygo.MenuItem {
		it := &mygo.MenuItem{ID: id, Label: t(label), Disabled: true}
		if key != 0 {
			it.Accelerator = Accelerator(mods, key)
		}
		return it
	}
	sep := mygo.Separator
	view := []*mygo.MenuItem{
		item("menu_search", "app.search"),
		off("menu_find_conversation", "conversation.find", ui.Cmd, ui.KeyF),
		sep(),
		item("menu_toggle_sidebar", "sidebar.toggle"),
		item("menu_toggle_details", "panel.toggle"),
		sep(),
		role("menu_full_screen", mygo.RoleToggleFullScreen, ui.Ctrl|ui.Super, ui.KeyF),
	}
	if len(a.features) > 0 {
		view = append(view, sep())
	}
	for _, id := range a.features {
		if f, ok := featureOf(id); ok {
			view = append(view, item(f.Label, f.ID))
		}
	}
	return []*mygo.MenuItem{
		{Label: t("app"), Submenu: []*mygo.MenuItem{
			item("menu_about", "app.about"),
			sep(),
			item("menu_settings", "app.settings"),
			sep(),
			role("menu_services", mygo.RoleServices, 0, 0),
			sep(),
			role("menu_hide", mygo.RoleHide, ui.Super, ui.KeyH),
			role("menu_hide_others", mygo.RoleHideOthers, ui.Super|ui.Alt, ui.KeyH),
			role("menu_show_all", mygo.RoleUnhide, 0, 0),
			sep(),
			role("menu_quit", mygo.RoleQuit, ui.Cmd, ui.KeyQ),
		}},
		{Label: t("menu_file"), Submenu: []*mygo.MenuItem{
			item("menu_new_conversation", "conversation.new"),
			item("menu_new_project", "project.new"),
			item("menu_open_folder", "folder.open"),
			sep(),
			off("menu_save", "file.save", ui.Cmd, ui.KeyS),
			sep(),
			item("menu_close_tab", CloseFocused.ID),
			role("menu_close_window", mygo.RoleClose, ui.Super|ui.Shift, ui.KeyW),
		}},
		{Label: t("menu_edit"), Submenu: []*mygo.MenuItem{
			role("menu_undo", mygo.RoleUndo, ui.Cmd, ui.KeyZ),
			role("menu_redo", mygo.RoleRedo, ui.Cmd|ui.Shift, ui.KeyZ),
			sep(),
			role("menu_cut", mygo.RoleCut, ui.Cmd, ui.KeyX),
			role("menu_copy", mygo.RoleCopy, ui.Cmd, ui.KeyC),
			role("menu_paste", mygo.RolePaste, ui.Cmd, ui.KeyV),
			role("menu_select_all", mygo.RoleSelectAll, ui.Cmd, ui.KeyA),
		}},
		{Label: t("menu_view"), Submenu: view},
		{Label: t("menu_window"), Submenu: []*mygo.MenuItem{
			role("menu_minimize", mygo.RoleMinimize, ui.Super, ui.KeyM),
			role("menu_zoom", mygo.RoleZoom, 0, 0),
			sep(),
			role("menu_bring_all_to_front", mygo.RoleFront, 0, 0),
		}},
		{Label: t("menu_help"), Submenu: []*mygo.MenuItem{
			item("menu_website", "help.website"),
			item("menu_feedback", "help.feedback"),
			off("menu_logs", "help.logs", 0, 0),
		}},
	}
}

// shortcutOf is the window's or ⌘W's shortcut by its ID.
func shortcutOf(id string) (Shortcut, bool) {
	if id == CloseFocused.ID {
		return CloseFocused, true
	}
	for _, s := range Shortcuts {
		if s.ID == id {
			return s, true
		}
	}
	return Shortcut{}, false
}

// Accelerator writes a shortcut as MyGo's menus take it: CmdOrCtrl+Alt+N
// for ⌥⌘N, ui.Cmd being Command on macOS and Ctrl elsewhere.
func Accelerator(mods ui.Modifiers, key ui.Key) string {
	var parts []string
	if mods&ui.Cmd != 0 {
		parts = append(parts, "CmdOrCtrl")
		mods &^= ui.Cmd
	}
	for _, m := range []struct {
		mod ui.Modifiers
		s   string
	}{{ui.Ctrl, "Ctrl"}, {ui.Alt, "Alt"}, {ui.Shift, "Shift"}, {ui.Super, "Super"}} {
		if mods&m.mod != 0 {
			parts = append(parts, m.s)
		}
	}
	return strings.Join(append(parts, kit.KeyName(key)), "+")
}
