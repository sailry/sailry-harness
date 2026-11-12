package shell

import (
	"slices"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
)

// A link of a demo conversation stays in the window: a web address opens a
// browser tab of the right panel and a file of the conversation its preview;
// only a web address clicked with the platform modifier reaches the system.
func testSampleLinks(t *testing.T) {
	a := New(preview.Load())
	a.Settings.Model.DemoMode = true
	var run func(c *ui.Context)
	tt := ui.NewTesterWith(func(c *ui.Context) {
		a.View(c)
		if f := run; f != nil {
			run = nil
			f(c)
		}
	}, 1280, 820, opaque)
	tt.SetLocale("en")
	tt.Frame()
	if !a.Demo() || a.Messages.OpenLink == nil {
		t.Fatal("the demo's transcript opens its links itself")
	}
	const session = "lumen-web/metrics"
	// A file the conversation attached or produced that is not a picture.
	path := ""
	for _, turn := range a.Data.Find(session).Turns {
		files := slices.Clone(turn.Files)
		for _, b := range turn.Reply.Blocks {
			files = append(files, b.Files...)
		}
		for _, f := range files {
			if path == "" && f.Kind != conversation.ImageFile {
				path = f.Path
			}
		}
	}
	if path == "" {
		t.Fatal("the demo conversation has no file")
	}
	open := func(mods ui.Modifiers, link string) (taken bool) {
		t.Helper()
		tt.HoldModifiers(mods)
		run = func(c *ui.Context) { taken = a.Messages.OpenLink(c, session, link) }
		tt.Frame()
		tt.HoldModifiers(0)
		tt.Frame()
		return taken
	}
	const page = "https://lumen.example/docs/metrics/retention"
	a.PanelOpen = false
	if !open(0, page) || a.Panel.Active.Kind != panel.Browser || !a.PanelOpen || len(tt.OpenedURLs()) != 0 {
		t.Fatalf("a web link: tab %+v, panel %v, system %q", a.Panel.Active, a.PanelOpen, tt.OpenedURLs())
	}
	tabs := len(a.Panel.Tabs)
	if !open(ui.Cmd, page) || len(a.Panel.Tabs) != tabs || !slices.Equal(tt.OpenedURLs(), []string{page}) {
		t.Fatalf("a web link with the modifier: %d tabs, system %q", len(a.Panel.Tabs), tt.OpenedURLs())
	}
	for _, mods := range []ui.Modifiers{0, ui.Cmd} {
		// Files open their preview in either mode: a sample path names
		// nothing on this device.
		a.Panel.Show(panel.Tab{Kind: panel.Files})
		if !open(mods, path) || a.Panel.Active.Kind != panel.Artifact {
			t.Errorf("file %q with modifiers %v opened %+v", path, mods, a.Panel.Active)
		}
		for _, link := range []string{"mailto:a@example.com", "https://user:pw@example.com/", "sailry://session/0b7c3e1a-5d2f-4c6b-9a8e-1f2d3c4b5a69", "no/such/file.go", "/etc/hosts"} {
			before := a.Panel.Active
			if !open(mods, link) || a.Panel.Active != before {
				t.Errorf("%q with modifiers %v opened %+v", link, mods, a.Panel.Active)
			}
		}
	}
	if open(0, "#heading") {
		t.Error("a heading link is not left to the transcript")
	}
	if got := tt.OpenedURLs(); !slices.Equal(got, []string{page}) {
		t.Errorf("the system opened %q", got)
	}
}
