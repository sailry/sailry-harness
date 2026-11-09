package panel

import (
	"slices"
	"testing"
)

func TestOpenedAddsOnce(t *testing.T) {
	tabs := Opened(Opened(nil, Files), Changes)
	if tabs = Opened(tabs, Files); !slices.Equal(tabs, []Kind{Files, Changes}) {
		t.Errorf("tabs %v", tabs)
	}
}

func TestClosedChoosesNext(t *testing.T) {
	tabs := []Kind{Files, Changes, Browser}
	if out, active := Closed(tabs, Browser, Files); active != Files || len(out) != 2 {
		t.Errorf("closing another kept %q, %v", active, out)
	}
	if _, active := Closed(tabs, Changes, Changes); active != Files {
		t.Errorf("closing the active chose %q, want the first", active)
	}
	if out, active := Closed([]Kind{Files}, Files, Files); len(out) != 0 || active != "" {
		t.Errorf("closing the last left %v %q", out, active)
	}
}
