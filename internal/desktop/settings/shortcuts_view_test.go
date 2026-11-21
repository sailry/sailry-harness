package settings

import (
	"image/png"
	"math"
	"os"
	"path/filepath"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// shortcutsPage is the Shortcuts page alone in the application's theme, tall
// enough that every row is in the window.
func shortcutsPage(k *Keys, mode theme.Mode, width int) *ui.Tester {
	tt := ui.NewTesterWith(func(c *ui.Context) {
		theme.Apply(c, "", mode)
		ui.Column(c).Fill().Padding(24).Background(theme.Of(c).Background).Children(func() { shortcuts(c, k) })
	}, width, 3200, ui.TesterOptions{Dark: mode == theme.Dark, Preferences: ui.Preferences{ReduceTransparency: true}})
	tt.SetLocale("en")
	tt.Frame()
	return tt
}

// focusedButton is the button with the keyboard focus, as assistive
// technology reads it; the zero AccessInfo when none has it.
func focusedButton(tt *ui.Tester) ui.AccessInfo {
	for _, a := range tt.AccessAll() {
		if a.Focused && a.Role == ui.RoleButton {
			return a
		}
	}
	return ui.AccessInfo{}
}

// keyButton is a global command's key button, named by the command.
func keyButton(tt *ui.Tester, id string) (ui.AccessInfo, bool) {
	s, _ := find(id)
	return tt.AccessByRole(ui.RoleButton, i18n.In("en", s.Label))
}

// nearKeySize reports whether two lengths differ by at most a DIP, as a face in
// the interface font beside a cap's monospace one.
func nearKeySize(a, b float32) bool { return math.Abs(float64(a-b)) <= 1 }

// scaled reports whether length is base zoomed by z, within the text
// metrics' rounding at another size: a cut or a wrapped cap is far off.
func scaled(length, base, z float32) bool {
	return math.Abs(float64(length-base*z)) <= max(1, 0.04*float64(base*z))
}

func inside(in, out ui.Rect) bool {
	return in.X >= out.X && in.Y >= out.Y && in.X+in.W <= out.X+out.W+0.5 && in.Y+in.H <= out.Y+out.H+0.5
}

// Every key is one cap of the same size: a global command's inside its
// focusable button, which assistive technology reads as the command with
// the key as its value, a fixed command's alternatives side by side and no
// button. An unset key and a reserved one are buttons of the same height,
// told apart by their face and the row's description. Narrow and zoomed,
// no cap leaves the window or is cut or wrapped.
func TestShortcutKeyCaps(t *testing.T) {
	k := NewKeys()
	k.SetOverrides(map[string]string{"app.search": "secondary-,"}) // Settings' key: reserved
	tt := shortcutsPage(&k, theme.Light, 760)
	en := func(key string) string { return i18n.In("en", key) }

	var texts []string
	var capH, buttonH float32
	for _, s := range Registry {
		ch := k.Chord(s)
		if ch.Key == 0 {
			continue
		}
		text := kit.KeyText(ch.Mods, ch.Key)
		b, ok := keyButton(tt, s.ID)
		key, shown := tt.Find(text)
		if !ok || b.Value != text || !shown || !b.Focusable || b.Bounds.H < keyButtonHeight || b.Bounds.W < keyButtonHeight || !inside(key, b.Bounds) {
			t.Fatalf("%s: button %+v %v, cap %+v %v", s.ID, b, ok, key, shown)
		}
		if capH == 0 {
			capH, buttonH = key.H, b.Bounds.H
		}
		if math.Abs(float64(key.H-capH)) > 0.001 || b.Bounds.H != buttonH {
			t.Errorf("%s: cap %v high in a %v button, want %v in %v", s.ID, key.H, b.Bounds.H, capH, buttonH)
		}
		texts = append(texts, text)
	}
	if !tt.HasText(en("shortcut_reserved")) {
		t.Errorf("the reserved key has no description: %q", tt.Texts())
	}
	if b, ok := keyButton(tt, "system.selection"); !ok || b.Value != en("shortcut_disabled") || !tt.HasText(b.Value) || !b.Focusable || !nearKeySize(b.Bounds.H, buttonH) {
		t.Errorf("the unset shortcut's button: %+v %v, want %v high", b, ok, buttonH)
	}
	if b, ok := keyButton(tt, "app.search"); !ok || b.Value != kit.KeyText(ui.Cmd, ui.KeyComma) {
		t.Errorf("the reserved shortcut's button: %+v %v", b, ok)
	}
	for _, l := range Listed {
		for _, ch := range l.Chords {
			text := kit.KeyText(ch.Mods, ch.Key)
			key, shown := tt.Find(text)
			button := false
			for _, a := range tt.AccessAll() {
				button = button || a.Role == ui.RoleButton && (a.Name == text || a.Value == text)
			}
			if !shown || button || math.Abs(float64(key.H-capH)) > 0.001 {
				t.Errorf("%s %s: shown %v, a button %v, %v high, want %v", l.Label, text, shown, button, key.H, capH)
			}
			texts = append(texts, text)
		}
	}

	at := map[string]ui.Rect{}
	for _, text := range texts {
		at[text], _ = tt.Find(text)
	}
	for _, size := range []struct {
		width int
		zoom  float32
	}{{420, 1}, {760, 2}, {560, 1.5}} {
		tt.SetSize(size.width, 4800)
		tt.SetZoom(size.zoom)
		tt.Frame()
		for _, text := range texts {
			r, ok := tt.Find(text)
			want := at[text]
			if !ok || r.X < 0 || r.X+r.W > float32(size.width)+0.5 ||
				!scaled(r.W, want.W, size.zoom) || !scaled(r.H, want.H, size.zoom) {
				t.Errorf("%d wide at %vx: %s is %+v, want %vx%v inside", size.width, size.zoom, text, r, want.W*size.zoom, want.H*size.zoom)
			}
		}
	}
}

// Recording keeps the button's place and size; a conflict and a key without
// a modifier, Backspace too, keep recording, Escape cancels, and each way out
// of recording, by key, reset or clear, returns the focus to the shortcut's
// button, where Enter records again.
func TestShortcutRecorderFocus(t *testing.T) {
	k := NewKeys()
	tt := shortcutsPage(&k, theme.Light, 760)
	en := func(key string) string { return i18n.In("en", key) }
	newChat, other := kit.KeyText(ui.Cmd, ui.KeyN), kit.KeyText(ui.Cmd|ui.Shift, ui.KeyN)
	name := en("menu_new_conversation")
	newConversation, _ := find("conversation.new")
	idle, _ := keyButton(tt, "conversation.new")
	if err := tt.Click(newChat); err != nil {
		t.Fatal(err)
	}
	rec, ok := keyButton(tt, "conversation.new")
	if k.Active != "conversation.new" || !ok || rec.Value != en("shortcut_record") || !tt.HasText(rec.Value) ||
		!nearKeySize(rec.Bounds.H, idle.Bounds.H) || !nearKeySize(rec.Bounds.Y, idle.Bounds.Y) {
		t.Fatalf("recording %q: %+v %v, idle %+v", k.Active, rec, ok, idle)
	}
	if reset, _ := tt.AccessByRole(ui.RoleButton, en("shortcut_reset")); !reset.Disabled {
		t.Error("reset is offered on a default key")
	}
	if clear, ok := tt.AccessByRole(ui.RoleButton, en("shortcut_clear")); !ok || clear.Disabled {
		t.Errorf("clear: %+v %v", clear, ok)
	}

	tt.Key(ui.Cmd, ui.KeyK)
	tt.Key(0, ui.KeyBackspace)
	tt.Key(0, ui.KeyDelete)
	if !k.Recording() || k.Chord(newConversation) != (Chord{ui.Cmd, ui.KeyN}) {
		t.Fatalf("a refused key ended recording %v or changed the key: %+v", k.Recording(), k.Chord(newConversation))
	}
	tt.Key(0, ui.KeyEscape)
	tt.Frame()
	if f := focusedButton(tt); k.Recording() || f.Name != name || f.Value != newChat {
		t.Fatalf("Escape: recording %v, focus on %+v", k.Recording(), f)
	}

	tt.Key(0, ui.KeyEnter)
	tt.Key(ui.Cmd|ui.Shift, ui.KeyN)
	tt.Frame()
	if f := focusedButton(tt); k.Recording() || k.Overrides()["conversation.new"] != "secondary-shift-n" || f.Name != name || f.Value != other || !tt.HasText(other) {
		t.Fatalf("taking a key: recording %v, %v, focus on %+v", k.Recording(), k.Overrides(), f)
	}

	tt.Key(0, ui.KeyEnter)
	if err := tt.Click(en("shortcut_reset")); err != nil {
		t.Fatal(err)
	}
	tt.Frame()
	if f := focusedButton(tt); k.Recording() || len(k.Overrides()) != 0 || f.Name != name || f.Value != newChat {
		t.Fatalf("reset: recording %v, %v, focus on %+v", k.Recording(), k.Overrides(), f)
	}

	tt.Key(0, ui.KeyEnter)
	if err := tt.Click(en("shortcut_clear")); err != nil {
		t.Fatal(err)
	}
	tt.Frame()
	if f := focusedButton(tt); k.Recording() || k.Chord(newConversation) != (Chord{}) || f.Name != name || f.Value != en("shortcut_disabled") {
		t.Fatalf("clear: recording %v, %+v, focus on %+v", k.Recording(), k.Chord(newConversation), f)
	}
	// Two unset shortcuts' buttons are told apart by their commands.
	selection, _ := keyButton(tt, "system.selection")
	if selection.Value != en("shortcut_disabled") || selection.Name == name {
		t.Errorf("the unset buttons read alike: %+v", selection)
	}
	tt.Key(0, ui.KeyEnter)
	if clear, _ := tt.AccessByRole(ui.RoleButton, en("shortcut_clear")); k.Active != "conversation.new" || !clear.Disabled {
		t.Errorf("recording an unset key: %q, clear %+v", k.Active, clear)
	}
}

// For review: $SAILRY_SNAPSHOTS/shortcuts-keys-<state>-<mode>.png at 2x, the
// page at rest, recording, and 420 wide at 150% zoom.
func TestShortcutKeyCapsSnapshots(t *testing.T) {
	dir := os.Getenv("SAILRY_SNAPSHOTS")
	if dir == "" {
		t.Skip("SAILRY_SNAPSHOTS is not set")
	}
	for _, mode := range []theme.Mode{theme.Light, theme.Dark} {
		name := map[theme.Mode]string{theme.Light: "light", theme.Dark: "dark"}[mode]
		for _, state := range []string{"rest", "recording", "narrow"} {
			k := NewKeys()
			k.SetOverrides(map[string]string{"app.search": "secondary-,", "sidebar.toggle": ""})
			width := 760
			if state == "narrow" {
				width = 420
			}
			tt := shortcutsPage(&k, mode, width)
			tt.SetLocale("zh-CN")
			tt.SetSize(width, 1250)
			tt.SetScale(2)
			if state == "recording" {
				k.Record("conversation.new")
			}
			if state == "narrow" {
				tt.SetSize(width, 1600)
				tt.SetZoom(1.5)
			}
			tt.Frame()
			f, err := os.Create(filepath.Join(dir, "shortcuts-keys-"+state+"-"+name+".png"))
			if err != nil {
				t.Fatal(err)
			}
			if err := png.Encode(f, tt.Image()); err != nil {
				t.Fatal(err)
			}
			f.Close()
		}
	}
}
