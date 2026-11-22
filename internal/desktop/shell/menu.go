package shell

import (
	"cmp"
	"strings"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
)

// Menu is the native app menu in lang: App, File, Edit, View, Window and
// Help, with the reference's items on the keys they have now (syncMenuKeys
// follows a rebound one), then the rail's features in View. The app's items send their command's ID through send, which
// the application passes to Do: the menu takes their keys before the
// window does. The system's items are MyGo's roles under the reference's
// labels and keys: the menu binds Close Window, Hide, Hide Others, Minimize
// and Full Screen on cmd (ui.Super), the rest on secondary (ui.Cmd). Minimize
// is the app's item on cmd: in a focused conversation its key enlarges the
// pane, elsewhere it minimizes the window (WindowMinimize). Save follows
// the focused live editor. Logs stays disabled without a live log folder
// (Hooks.OpenLogs). Help also opens the welcome guide.
func (a *App) Menu(lang string, send func(id string)) []*mygo.MenuItem {
	name := cmp.Or(a.Settings.ProductName, a.translations.In(lang, "app"))
	t := func(key string) string { return a.translations.In(lang, key, "app", name) }
	a.menuKeys = nil
	item := func(label, id string) *mygo.MenuItem {
		it := &mygo.MenuItem{ID: id, Label: t(label), Click: func(*mygo.MenuItem, *mygo.Window) { send(id) }}
		if s, ok := shortcutOf(id); ok {
			key := menuKey{item: it, shortcut: s, chord: a.Settings.Keys.Effective(s)}
			it.Accelerator = key.accelerator()
			a.menuKeys = append(a.menuKeys, key)
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
	save := off("menu_save", "file.save", ui.Cmd, ui.KeyS)
	if !a.Preview {
		save.Disabled = !a.fileSaveEnabled
		save.Click = func(*mygo.MenuItem, *mygo.Window) {
			if a.fileSaveEnabled {
				send("file.save")
			}
		}
		a.fileSaveMenu = save.SetEnabled
	}
	logs := off("menu_logs", "help.logs", 0, 0)
	if !a.Preview && a.Hooks.OpenLogs != nil {
		logs.Disabled = false
		logs.Click = func(*mygo.MenuItem, *mygo.Window) { send("help.logs") }
	}
	view := []*mygo.MenuItem{
		item("menu_search", "app.search"),
		item("menu_find_conversation", "conversation.find"),
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
		if f, ok := a.lookupFeature(id); ok {
			view = append(view, item(f.Label, f.ID))
		}
	}
	var help []*mygo.MenuItem
	if a.Website != "" {
		help = append(help, item("menu_website", "help.website"))
	}
	if a.Feedback != "" {
		help = append(help, item("menu_feedback", "help.feedback"))
	}
	help = append(help, logs)
	if a.hasWelcome() {
		help = append(help, sep(), item("onboarding_title", "app.welcome"))
	}
	return []*mygo.MenuItem{
		{Label: name, Submenu: []*mygo.MenuItem{
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
			save,
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
			{ID: WindowMinimize, Label: t("menu_minimize"), Accelerator: Accelerator(ui.Super, ui.KeyM), Click: func(_ *mygo.MenuItem, w *mygo.Window) {
				a.minimizeWindow = nil
				if w != nil {
					a.minimizeWindow = w.Minimize
				}
				send(WindowMinimize)
			}},
			role("menu_zoom", mygo.RoleZoom, 0, 0),
			sep(),
			role("menu_bring_all_to_front", mygo.RoleFront, 0, 0),
		}},
		{Label: t("menu_help"), Submenu: help},
	}
}

// menuKey is a menu item that shows its command's key: the user's, none
// when it is off.
type menuKey struct {
	item     *mygo.MenuItem
	shortcut Shortcut
	chord    settings.Chord // the key the item shows
}

func (k menuKey) accelerator() string {
	if k.chord.Key == 0 {
		return ""
	}
	return Accelerator(k.chord.Mods, k.chord.Key)
}

// syncMenuKeys moves the menu's items to their commands' keys of now, each
// frame: a rebound key leaves the menu with the old one otherwise, which the
// menu takes before the window does.
func (a *App) syncMenuKeys() {
	for i := range a.menuKeys {
		k := &a.menuKeys[i]
		if now := a.Settings.Keys.Effective(k.shortcut); now != k.chord {
			k.chord = now
			k.item.SetAccelerator(k.accelerator())
		}
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
