package crychic

import (
	"slices"
	"strings"
	"testing"

	"github.com/ELY-Open-Source/CrychicUI/navigation"
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// Crumbs cannot supply PathBar: its overflow is its own left-click menu,
// long individual segments overflow their row, and its last segment has no
// action. Keep the probe alongside the components approved for other uses.
func TestCrumbs(t *testing.T) {
	t.Run("folds paths with an owned menu and localized copy", func(t *testing.T) {
		chosen := -1
		s := newStage(func(c *ui.Context) {
			navigation.Crumbs(c, "t:path", []string{"t:root", "t:one", "t:two", "t:three", "t:last"}, &chosen, navigation.CrumbsOptions{More: "t:hidden"})
		}, theme.Light)
		s.settle()
		if s.tt.HasText("t:one") || s.tt.HasText("t:two") || !s.tt.HasText("t:root") || !s.tt.HasText("t:last") {
			t.Fatalf("path folding changed: %q", s.tt.Texts())
		}
		for _, said := range s.said() {
			if strings.Contains(said, "More") {
				t.Fatalf("Crumbs uses untranslated copy: %q", said)
			}
		}
		if err := s.tt.Click("t:hidden"); err != nil {
			t.Fatal(err)
		}
		if menu := s.tt.Menu(); !slices.Equal(menu, []string{"t:one", "t:two"}) {
			t.Fatalf("Crumbs no longer owns the hidden-segment menu: %q", menu)
		}
	})
	t.Run("long segments overflow and the last segment has no action", func(t *testing.T) {
		chosen := -1
		long := strings.Repeat("directory", 20)
		s := newStage(func(c *ui.Context) {
			ui.Column(c).Width(180).Children(func() {
				navigation.Crumbs(c, "t:path", []string{long, "t:last"}, &chosen, navigation.CrumbsOptions{More: "t:hidden"})
			})
		}, theme.Light)
		s.settle()
		root, exists := s.tt.Access("t:path")
		longBounds, shown := s.tt.Find(long)
		if !exists || !shown || longBounds.X+longBounds.W <= root.Bounds.X+root.Bounds.W {
			t.Fatalf("Crumbs no longer overflows a long label; re-evaluate PathBar: path=%v segment=%v", root.Bounds, longBounds)
		}
		if _, exists := s.tt.AccessByRole(ui.RoleButton, "t:last"); exists {
			t.Fatal("Crumbs now makes the final segment actionable; re-evaluate PathBar")
		}
		if chosen != -1 {
			t.Fatalf("Crumbs changed the selected segment without input: %d", chosen)
		}
	})
	t.Run("MyGo keeps the final segment noninteractive", func(t *testing.T) {
		chosen := -1
		s := newStage(func(c *ui.Context) { ui.Breadcrumbs(c, []string{"t:root", "t:last"}, &chosen).Label("t:path") }, theme.Light)
		s.settle()
		if err := s.tt.Click("t:root"); err != nil {
			t.Fatal(err)
		}
		if chosen != 0 {
			t.Fatalf("MyGo did not select the ancestor: %d", chosen)
		}
		if _, exists := s.tt.AccessByRole(ui.RoleLink, "t:last"); exists {
			t.Fatal("MyGo now makes the final segment actionable; re-evaluate PathBar")
		}
	})
}
