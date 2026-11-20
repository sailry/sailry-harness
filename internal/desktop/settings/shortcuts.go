package settings

import (
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// Chord is a key with its modifiers; the zero Chord is no key.
type Chord struct {
	Mods ui.Modifiers
	Key  ui.Key
}

// Shortcut is a command's key: its ID, its label's i18n key and its default
// key, ⌘ standing for Ctrl off macOS. System ones act outside the window,
// registered with the system.
type Shortcut struct {
	ID, Label string
	Mods      ui.Modifiers
	Key       ui.Key
	System    bool
}

// Registry is the shortcuts the Shortcuts page lists and lets the user
// rebind.
var Registry = []Shortcut{
	{ID: "conversation.new", Label: "menu_new_conversation", Mods: ui.Cmd, Key: ui.KeyN},
	{ID: "project.new", Label: "menu_new_project", Mods: ui.Cmd | ui.Alt, Key: ui.KeyN},
	{ID: "folder.open", Label: "menu_open_folder", Mods: ui.Cmd, Key: ui.KeyO},
	{ID: "app.search", Label: "search", Mods: ui.Cmd, Key: ui.KeyK},
	{ID: "sidebar.toggle", Label: "sidebar_toggle", Mods: ui.Cmd, Key: ui.KeyB},
	{ID: "panel.toggle", Label: "shortcut_inspector", Mods: ui.Cmd | ui.Alt, Key: ui.KeyB},
	{ID: "host.refresh", Label: "shortcut_sync", Mods: ui.Cmd | ui.Shift, Key: ui.KeyR},
	{ID: "conversation.previous", Label: "shortcut_previous_conversation", Mods: ui.Cmd | ui.Shift, Key: ui.KeyBracketLeft},
	{ID: "conversation.next", Label: "shortcut_next_conversation", Mods: ui.Cmd | ui.Shift, Key: ui.KeyBracketRight},
	{ID: "app.shortcuts", Label: "settings_shortcuts", Mods: ui.Cmd, Key: ui.KeySlash},
	{ID: "system.activate", Label: "shortcut_activate", Mods: ui.Ctrl | ui.Alt, Key: ui.KeySpace, System: true},
	{ID: "system.selection", Label: "shortcut_selection", System: true},
}

// Fixed are keys of the app no shortcut may take: Settings, Find in
// Conversation, Close Tab, Close Window and Quit, then the conversation's
// Stop, Focus the composer and model controls, then the panes' and the view's
// keys (Listed).
var Fixed = append([]Chord{{ui.Cmd, ui.KeyComma}, {ui.Cmd, ui.KeyF}, {ui.Cmd, ui.KeyW}, {ui.Cmd | ui.Shift, ui.KeyW}, {ui.Cmd, ui.KeyQ},
	{ui.Cmd, ui.KeyPeriod}, {ui.Cmd, ui.KeyL}, {ui.Cmd | ui.Shift, ui.KeyM}}, listedChords()...)

// ListedKeys is a fixed command the Shortcuts page shows without recording:
// its label's i18n key and its keys.
type ListedKeys struct {
	Label  string
	Chords []Chord
}

// Listed are the conversation panes' keys, which act in the focused
// conversation (a split's pane or the conversation standing alone), and the
// view zoom's, which act in the whole window.
var Listed = []ListedKeys{
	{"shortcut_pane_split", []Chord{{ui.Cmd | ui.Shift, ui.KeyRight}, {ui.Cmd | ui.Shift, ui.KeyLeft}, {ui.Cmd | ui.Shift, ui.KeyUp}, {ui.Cmd | ui.Shift, ui.KeyDown}}},
	{"shortcut_pane_tab", []Chord{{ui.Cmd, ui.KeyT}}},
	{"shortcut_pane_enlarge", []Chord{{ui.Cmd, ui.KeyM}}},
	{"shortcut_zoom_in", []Chord{{ui.Cmd, ui.KeyEqual}, {ui.Cmd | ui.Shift, ui.KeyEqual}}},
	{"shortcut_zoom_out", []Chord{{ui.Cmd, ui.KeyMinus}}},
	{"shortcut_zoom_reset", []Chord{{ui.Cmd, ui.Key0}}},
}

func listedChords() []Chord {
	var out []Chord
	for _, l := range Listed {
		out = append(out, l.Chords...)
	}
	return out
}

// fixed reports whether a key is one of the app's own.
func fixed(ch Chord) bool {
	for _, f := range Fixed {
		if f == ch {
			return true
		}
	}
	return false
}

// Keys is the Shortcuts page's state: the user's keys over the defaults, and
// the shortcut being recorded.
type Keys struct {
	set     map[string]Chord // a zero Chord turns the shortcut off
	Active  string           // the shortcut being recorded, "" for none
	problem string           // a refused key's i18n key, toasted next frame
	focus   ui.Handle
}

// NewKeys has every shortcut on its default key.
func NewKeys() Keys { return Keys{set: map[string]Chord{}} }

// Chord is a shortcut's key now.
func (k *Keys) Chord(s Shortcut) Chord {
	if ch, ok := k.set[s.ID]; ok {
		return ch
	}
	return Chord{s.Mods, s.Key}
}

// Reserved reports whether the key the user gave s is now one of the app's
// own (Fixed), as a key saved before the app took it. The key is kept, and
// the page shows it as reserved until the user chooses another.
func (k *Keys) Reserved(s Shortcut) bool {
	_, set := k.set[s.ID]
	return set && fixed(k.Chord(s))
}

// Effective is the key s runs on: its key now, none when turned off or
// reserved, so the app's own command alone takes a reserved key.
func (k *Keys) Effective(s Shortcut) Chord {
	if k.Reserved(s) {
		return Chord{}
	}
	return k.Chord(s)
}

// Bound is s on the key it runs on; false when the user turned it off or its
// key is reserved.
func (k *Keys) Bound(s Shortcut) (Shortcut, bool) {
	ch := k.Effective(s)
	s.Mods, s.Key = ch.Mods, ch.Key
	return s, ch.Key != 0
}

// Recording reports whether a shortcut is being recorded, which holds the
// window's shortcuts so the keys reach the recorder.
func (k *Keys) Recording() bool { return k.Active != "" }

// Record starts recording the shortcut id.
func (k *Keys) Record(id string) { k.Active, k.problem = id, "" }

// Cancel stops recording, keeping the key.
func (k *Keys) Cancel() { k.Active = "" }

// Reset puts the shortcut being recorded back on its default.
func (k *Keys) Reset() {
	delete(k.set, k.Active)
	k.Active = ""
}

// Clear turns the shortcut being recorded off.
func (k *Keys) Clear() {
	k.set[k.Active] = Chord{}
	k.Active = ""
}

// Capture takes a key for the shortcut being recorded. It returns the i18n
// key of the problem, "" when it took it: shortcut_invalid for a key without
// a modifier, shortcut_conflict for a key another shortcut or the app has. A
// refused key keeps recording.
func (k *Keys) Capture(ch Chord) string {
	s, ok := find(k.Active)
	if !ok {
		return "shortcut_unavailable"
	}
	if problem := k.check(s, ch); problem != "" {
		return problem
	}
	if ch == (Chord{s.Mods, s.Key}) {
		delete(k.set, s.ID)
	} else {
		k.set[s.ID] = ch
	}
	k.Active = ""
	return ""
}

// Take is Capture for a key that comes from outside the page, as a menu
// item's: a problem toasts as the page shows next.
func (k *Keys) Take(ch Chord) { k.problem = k.Capture(ch) }

func (k *Keys) check(s Shortcut, ch Chord) string {
	function := false
	for _, f := range []ui.Key{ui.KeyF1, ui.KeyF2, ui.KeyF3, ui.KeyF4, ui.KeyF5, ui.KeyF6, ui.KeyF7, ui.KeyF8, ui.KeyF9, ui.KeyF10, ui.KeyF11, ui.KeyF12} {
		function = function || ch.Key == f
	}
	if ch.Key == 0 || ch.Key == ui.KeyUnknown || ch.Mods&(ui.Cmd|ui.Ctrl|ui.Alt|ui.Super) == 0 && !function {
		return "shortcut_invalid"
	}
	if fixed(ch) {
		return "shortcut_conflict"
	}
	for _, o := range Registry {
		if o.ID != s.ID && k.Chord(o) == ch {
			return "shortcut_conflict"
		}
	}
	return ""
}

func find(id string) (Shortcut, bool) {
	for _, s := range Registry {
		if s.ID == id {
			return s, true
		}
	}
	return Shortcut{}, false
}

// shortcuts is the global commands, each a row whose button shows its key and
// records a new one when clicked. While recording, the page takes every key
// (capture_key_down): Escape cancels, another key is saved, and the reset and
// clear buttons follow the button. Kit has no shortcut recorder; this is
// MyGo's buttons over a focusable column taking raw keys (HandleInput).
func shortcuts(c *ui.Context, k *Keys) {
	if k.problem != "" {
		kit.Toast(c, kit.Failed, i18n.T(c, k.problem))
		k.problem = ""
	}
	page := ui.Column(c.Key("shortcuts")).Focusable().FocusRing(false).Bind(&k.focus).HandleInput(func(ev ui.InputEvent) bool {
		if !k.Recording() || ev.Kind != ui.InputKeyDown {
			return false
		}
		switch {
		case ev.Key == ui.KeyEscape && ev.Mods == 0:
			k.Cancel()
		case ev.Repeat, ev.Key == ui.KeyUnknown:
		default:
			k.problem = k.Capture(Chord{ev.Mods, ev.Key})
		}
		return true
	})
	// The recorder keeps the keys when a click elsewhere, as on a toast's
	// close button, took the focus.
	if k.Recording() && !k.focus.FocusWithin(c) {
		k.focus.Focus(c)
	}
	page.Children(func() {
		kit.Group(c, "commands", i18n.T(c, "settings_global_commands"), "", nil, func() {
			kit.Card(c, func() {
				for _, s := range Registry {
					description := ""
					if k.Reserved(s) && k.Active != s.ID {
						description = i18n.T(c, "shortcut_reserved")
					}
					kit.Row(c, s.ID, i18n.T(c, s.Label), description, func() { shortcutControl(c, k, s) })
				}
			})
		})
		kit.Group(c, "panes", i18n.T(c, "settings_pane_commands"), "", nil, func() {
			kit.Card(c, func() {
				for _, l := range Listed {
					kit.Row(c, l.Label, i18n.T(c, l.Label), "", func() { listedControl(c, l) })
				}
			})
		})
	})
}

// listedControl is a fixed command's keys, as text: they record nothing.
func listedControl(c *ui.Context, l ListedKeys) {
	var keys []string
	for _, ch := range l.Chords {
		keys = append(keys, kit.KeyText(ch.Mods, ch.Key))
	}
	ui.Text(c, strings.Join(keys, "  ")).FontSize(12).SingleLine().TextColor(theme.Of(c).MutedForeground)
}

// shortcutControl is a shortcut's button, primary while recording, with
// reset and clear beside it then.
func shortcutControl(c *ui.Context, k *Keys, s Shortcut) {
	p := theme.Of(c)
	recording := k.Active == s.ID
	ch := k.Chord(s)
	label := i18n.T(c, "shortcut_disabled")
	switch {
	case recording:
		label = i18n.T(c, "shortcut_record")
	case ch.Key != 0:
		label = kit.KeyText(ch.Mods, ch.Key)
	}
	reserved := !recording && k.Reserved(s)
	var b ui.Element
	if recording {
		b = kit.Primary(c.Key("key"), label, false).Children(func() { ui.Text(c, label).FontSize(12) })
	} else {
		b = kit.Secondary(c.Key("key"), label)
		if ch.Key == 0 || reserved {
			b.TextColor(p.MutedForeground)
		}
	}
	b.OnClick(func() {
		k.Record(s.ID)
		k.focus.Focus(c)
	})
	if !recording {
		return
	}
	kit.IconButton(c.Key("reset"), icons.RotateCw, i18n.T(c, "shortcut_reset"), 32, false).OnClick(k.Reset)
	kit.IconButton(c.Key("clear"), icons.Close, i18n.T(c, "shortcut_clear"), 32, false).Disabled(ch.Key == 0).OnClick(k.Clear)
}
