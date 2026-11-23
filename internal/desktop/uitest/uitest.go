package uitest

import (
	"sync"

	"github.com/egoist/mygo/ui"
)

// Scale is the device pixel ratio used by non-pixel desktop tests to cut
// software rasterization cost while keeping DIP layout and hit-testing exact.
const Scale float32 = 0.125

// Quiet disables translucency and motion for tests that do not check frost or
// animations.
var Quiet = ui.TesterOptions{
	Preferences: ui.Preferences{ReduceTransparency: true, ReduceMotion: true},
}

// initMu serializes ui.NewTesterWith runtime creation because mygo@v0.10.0's
// newRuntime writes package-global frameStatsOn without synchronization when
// MYGO_FRAME_STATS is unset. initOnce warms up mygo's lazy linkGroups/linkShadows
// function pointers on a 1x1 frame before parallel painters read them.
var (
	initMu   sync.Mutex
	initOnce sync.Once
)

// New creates a ui.Tester with scale (1 when <= 0) and locale applied before
// view's first frame, keeping parallel tests race-free under -race and
// avoiding an initial 1x rasterization pass.
func New(view func(*ui.Context), width, height int, scale float32, locale string, opts ui.TesterOptions) *ui.Tester {
	initOnce.Do(func() {
		initMu.Lock()
		_ = ui.NewTesterWith(func(c *ui.Context) {
			ui.Box(c).Size(1, 1).Opacity(0.5).Shadow(0, 1, 1, 0, ui.RGBA(0, 0, 0, 1))
		}, 1, 1, Quiet)
		initMu.Unlock()
	})
	ready := false
	initMu.Lock()
	tt := ui.NewTesterWith(func(c *ui.Context) {
		if !ready {
			return
		}
		view(c)
	}, 1, 1, opts)
	initMu.Unlock()
	if scale > 0 && scale != 1 {
		tt.SetScale(scale)
	}
	if locale != "" {
		tt.SetLocale(locale)
	}
	ready = true
	tt.SetSize(width, height)
	return tt
}

// Fast creates a quiet tester at Scale for tests that do not inspect pixels or
// animation frames.
func Fast(view func(*ui.Context), width, height int, locale string) *ui.Tester {
	return New(view, width, height, Scale, locale, Quiet)
}
