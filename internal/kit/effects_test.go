package kit

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/theme"
)

// Theme effects ask for frames while they show, and none with Reduce
// Motion, the Appearance switch off or a hidden window.
func TestEffectsFollowMotion(t *testing.T) {
	tt := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "great-sage", theme.Dark)
		Draws(ui.Column(c).Fill(), Backdrop(c, "background.window"))
	}, 400, 300)
	settle := func() {
		tt.Frame()
		tt.Frame()
	}
	settle()
	if !tt.FramePending() {
		t.Fatal("effects ask for no frame")
	}
	tt.SetPreferences(ui.Preferences{ReduceMotion: true})
	settle()
	if tt.FramePending() {
		t.Error("effects animate with Reduce Motion")
	}
	tt.SetPreferences(ui.Preferences{})
	SetEffects(false)
	settle()
	if tt.FramePending() {
		t.Error("effects animate when turned off")
	}
	SetEffects(true)
	settle()
	if !tt.FramePending() {
		t.Error("effects do not resume")
	}
	tt.SetWindowHidden(true)
	settle()
	if tt.FramePending() {
		t.Error("effects animate in a hidden window")
	}
}

// The same seed and phase place the same particles.
func TestEffectsAreDeterministic(t *testing.T) {
	if sample(3, 11, 0.4) != sample(3, 11, 0.4) {
		t.Error("a particle moves between runs")
	}
}
