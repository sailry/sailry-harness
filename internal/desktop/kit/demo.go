package kit

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
)

// Demo data shows as the application's own does, with nothing that names it
// a sample: the window says whether it shows the demo, and the controls that
// identify preview data stay silent over it.

type demoKey struct{}

func demoOf(c *ui.Context) *bool {
	return ui.Local(c.Root(), demoKey{}, func() bool { return false })
}

// SetDemo records on the window root whether the frame shows demo data.
func SetDemo(c *ui.Context, on bool) {
	if demo := demoOf(c); demo != nil {
		*demo = on
	}
}

// Demo reports whether the window shows demo data.
func Demo(c *ui.Context) bool {
	demo := demoOf(c)
	return demo != nil && *demo
}

// PreviewNotice tells that an action stays within the preview. Over demo
// data it says nothing: the action changed the demo's memory, or nothing.
func PreviewNotice(c *ui.Context) {
	if !Demo(c) {
		Message(c, i18n.T(c, "preview_notice"))
	}
}
