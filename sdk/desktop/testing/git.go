package testing

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/kit/kittest"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
)

func PreviewProject(demo bool) *panel.Project {
	if demo {
		return panel.NewProjects(preview.Demo("en", "local").Workspace).Of("harbor-api")
	}
	return panel.NewProjects(preview.Load().Workspace).Of("sailry-go")
}

func RenderWorkspacePanel(c *ui.Context, state *panel.State, project *panel.Project) {
	panel.View(c, state, project, kit.KeyText)
}

func BindWorkspaceLayout(c *ui.Context, split *bool, share *float32) {
	kit.BindDiffSplit(c, split)
	if share != nil {
		panel.BindListShare(c, share)
	}
}

func AssertCentered(t *testing.T, window *ui.Tester, label string, area ui.Rect) {
	kittest.Centred(t, window, label, area)
}

func SetDemo(c *ui.Context, demo bool) { kit.SetDemo(c, demo) }
