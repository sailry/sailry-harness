package settings

import "github.com/egoist/mygo/ui"

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
	// buttons are the shortcuts' key buttons; refocus is the one that takes
	// the focus back when the page ends its recording, and landed the one
	// whose new key presses its cap once.
	buttons         map[string]*ui.Handle
	refocus, landed string
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
