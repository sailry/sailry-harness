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

// Each open item is its own tab: Files and Git once, browser pages and
// terminals each new; a kind the panel no longer opens is not shown.
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
