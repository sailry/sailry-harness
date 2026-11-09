package panel

import (
	"slices"
	"testing"

	"github.com/sailry/sailry-go/internal/preview"
)

func TestOpenedAddsOnce(t *testing.T) {
	files, changes := Tab{Kind: Files}, Tab{Kind: Changes}
	tabs := Opened(Opened(nil, files), changes)
	if tabs = Opened(tabs, files); !slices.Equal(tabs, []Tab{files, changes}) {
		t.Errorf("tabs %v", tabs)
	}
}

func TestClosedChoosesNext(t *testing.T) {
	files, changes, page := Tab{Kind: Files}, Tab{Kind: Changes}, Tab{Browser, "1"}
	tabs := []Tab{files, changes, page}
	if out, active := Closed(tabs, page, files); active != files || len(out) != 2 {
		t.Errorf("closing another kept %v, %v", active, out)
	}
	if _, active := Closed(tabs, changes, changes); active != files {
		t.Errorf("closing the active chose %v, want the first", active)
	}
	if out, active := Closed([]Tab{files}, files, files); len(out) != 0 || active != (Tab{}) {
		t.Errorf("closing the last left %v %v", out, active)
	}
}

// Each open item is its own tab: Files and Git once without a project,
// browser pages and terminals each new; a kind the panel no longer opens
// is not shown.
func TestFlatTabs(t *testing.T) {
	s := New(preview.Load().Workspace)
	s.Open(Files)
	s.Open(Changes)
	s.Show(Tab{"file", "a.go"})
	s.Open(Browser)
	s.New(Browser)
	s.Open(Files)
	if len(s.Tabs) != 4 || s.Active != (Tab{Kind: Files}) {
		t.Fatalf("tabs %v, active %v", s.Tabs, s.Active)
	}
	if s.Tabs[2] == s.Tabs[3] || s.Tabs[3].Kind != Browser {
		t.Errorf("a new browser page reused a tab: %v", s.Tabs)
	}
}

// Each open file and diff is its own tab, the same items as the page's
// strip; another project's items replace them.
func TestItemTabs(t *testing.T) {
	w := preview.Load().Workspace
	ps := NewProjects(w)
	one := ps.Of("one")
	s := New(w)
	s.Sync(one)
	s.Open(Files)
	s.Show(Tab{Files, "a.go"})
	s.Show(Tab{Files, "b.go"})
	s.Open(Changes)
	s.Sync(one)
	readme, diff := Tab{Files, "sailry-go/README.md"}, Tab{Changes, w.Changes[0].Path}
	if want := []Tab{readme, {Files, "a.go"}, {Files, "b.go"}, diff}; !slices.Equal(s.Tabs, want) || s.Active != diff {
		t.Fatalf("tabs %v, active %v", s.Tabs, s.Active)
	}
	if !slices.Equal(one.Files.Open(), []string{"sailry-go/README.md", "a.go", "b.go"}) || one.Files.File() != "b.go" {
		t.Fatalf("the project's files %v, shown %q", one.Files.Open(), one.Files.File())
	}
	// Closing a tab closes the item; one closed on the page drops its tab.
	s.Close(Tab{Files, "a.go"})
	one.Files.Close("b.go")
	s.Sync(one)
	if want := []Tab{readme, diff}; !slices.Equal(s.Tabs, want) || one.Files.File() != "sailry-go/README.md" {
		t.Fatalf("after closing: tabs %v, shown %q", s.Tabs, one.Files.File())
	}
	// Another project swaps the item tabs for its own.
	two := ps.Of("two")
	two.Files.Show("c.go")
	s.Sync(two)
	if want := []Tab{{Files, "c.go"}}; !slices.Equal(s.Tabs, want) || s.Active != want[0] {
		t.Fatalf("project two: tabs %v, active %v", s.Tabs, s.Active)
	}
}

func TestBrowserHistory(t *testing.T) {
	b := &page{at: -1}
	if b.Go("  ") || b.at != -1 {
		t.Fatal("an empty address went")
	}
	b.Go("sailry.dev")
	b.Go("https://example.com/a")
	b.Step(-1)
	if b.address != "https://sailry.dev" {
		t.Fatalf("back to %q", b.address)
	}
	b.Go("example.org")
	b.Step(1)
	if len(b.history) != 2 || b.address != "https://example.org" {
		t.Fatalf("history %v at %q", b.history, b.address)
	}
}
