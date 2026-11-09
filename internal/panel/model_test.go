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

// Each open item is its own tab: files and diffs by path, browser pages
// and terminals each new, Files and Git once.
func TestFlatTabs(t *testing.T) {
	s := New(preview.Load().Workspace)
	s.Open(Files)
	s.Show(Tab{File, "a.go"})
	s.Show(Tab{File, "b.go"})
	s.Show(Tab{Diff, "a.go"})
	s.Open(Browser)
	s.New(Browser)
	s.Open(Files)
	s.Show(Tab{File, "a.go"})
	if len(s.Tabs) != 6 || s.Active != (Tab{File, "a.go"}) {
		t.Fatalf("tabs %v, active %v", s.Tabs, s.Active)
	}
	if s.Tabs[4] == s.Tabs[5] || s.Tabs[5].Kind != Browser {
		t.Errorf("a new browser page reused a tab: %v", s.Tabs)
	}
}
