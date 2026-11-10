package kit

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// TestEffects groups the effect simulation; each case keeps its own fixture.
func TestEffects(t *testing.T) {
	t.Run("EffectsFollowMotion", testEffectsFollowMotion)
	t.Run("EffectsAreDeterministic", testEffectsAreDeterministic)
	t.Run("EffectSimulation", testEffectSimulation)
}

// Theme effects ask for frames while they show, and none with Reduce
// Motion, the Appearance switch off or a hidden window.
func testEffectsFollowMotion(t *testing.T) {
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
func testEffectsAreDeterministic(t *testing.T) {
	if sample(3, 11, 0.4) != sample(3, 11, 0.4) {
		t.Error("a particle moves between runs")
	}
}

func testEffectSimulation(t *testing.T) {
	t.Parallel()
	for _, scenario := range []string{"varies_with_seed", "remains_bounded", "loops_without_position_jump", "hides_travel_endpoints"} {
		t.Run(scenario, func(t *testing.T) {
			switch scenario {
			case "varies_with_seed":
				first, other := sample(1, 1, 0.2), sample(1, 2, 0.2)
				if first.x == other.x && first.y == other.y {
					t.Fatal("seed did not change position")
				}
			case "remains_bounded":
				for _, seed := range []uint32{0, 12, ^uint32(0)} {
					for index := range uint32(64) {
						for step := range 100 {
							point := sample(index, seed, float32(step)/100)
							for _, value := range []float32{point.x, point.y, point.time, point.fade, point.pulse} {
								if value < 0 || value > 1 {
									t.Fatalf("unbounded motion: %+v", point)
								}
							}
							if point.scale < 0.65 || point.scale > 1.35 {
								t.Fatalf("unbounded scale: %f", point.scale)
							}
						}
					}
				}
			case "loops_without_position_jump":
				for index := range uint32(64) {
					first, last := sample(index, 27, 0), sample(index, 27, 1)
					for _, pair := range [][2]float32{{first.x, last.x}, {first.y, last.y}, {first.time, last.time}, {first.fade, last.fade}, {first.pulse, last.pulse}, {first.sway, last.sway}} {
						if difference := pair[0] - pair[1]; difference < -0.00001 || difference > 0.00001 {
							t.Fatalf("motion loop jumped: %v", pair)
						}
					}
				}
			case "hides_travel_endpoints":
				for index := range uint32(64) {
					offset := sample(index, 27, 0).time
					if sample(index, 27, -offset).fade != 0 || sample(index, 27, 1-offset-0.00001).fade >= 0.00001 {
						t.Fatal("travel endpoint remained visible")
					}
				}
			}
		})
	}
}
