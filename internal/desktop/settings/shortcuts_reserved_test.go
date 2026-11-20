package settings

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

// The panes' and the view's keys are the app's own: recording refuses them,
// and a key saved on one before stays saved but is not bound.
func TestReservedKeys(t *testing.T) {
	k := NewKeys()
	newChat, _ := find("conversation.new")
	for _, l := range Listed {
		for _, ch := range l.Chords {
			k.Record(newChat.ID)
			if got := k.Capture(ch); got != "shortcut_conflict" || !k.Recording() {
				t.Errorf("%s %+v: %q, recording %v", l.Label, ch, got, k.Recording())
			}
			k.Cancel()
		}
	}
	k.SetOverrides(map[string]string{"sidebar.toggle": "secondary-="})
	toggle, _ := find("sidebar.toggle")
	if !k.Reserved(toggle) || k.Effective(toggle) != (Chord{}) {
		t.Fatalf("a saved Cmd+= is reserved %v, effective %+v", k.Reserved(toggle), k.Effective(toggle))
	}
	if _, ok := k.Bound(toggle); ok {
		t.Error("a reserved key is bound")
	}
	if k.Chord(toggle) != (Chord{ui.Cmd, ui.KeyEqual}) || k.Overrides()["sidebar.toggle"] != "secondary-=" {
		t.Errorf("the saved key changed: %+v, %q", k.Chord(toggle), k.Overrides()["sidebar.toggle"])
	}
	if k.Reserved(newChat) {
		t.Error("a default key is reserved")
	}
	// Choosing another key ends the reservation.
	k.Record(toggle.ID)
	if got := k.Capture(Chord{ui.Cmd | ui.Shift, ui.KeyB}); got != "" || k.Reserved(toggle) {
		t.Errorf("rebinding a reserved shortcut: %q, reserved %v", got, k.Reserved(toggle))
	}
}
