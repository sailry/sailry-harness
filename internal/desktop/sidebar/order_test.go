package sidebar

import (
	"slices"
	"testing"
	"time"
)

func ids(pins []Pin) []string {
	var out []string
	for _, p := range pins {
		out = append(out, p.ID)
	}
	return out
}

// TestOrder groups the sidebar's pure ordering rules; each case keeps its own fixture.
func TestOrder(t *testing.T) {
	t.Run("OrdersLegacyPinsBeforeNewPins", testOrdersLegacyPinsBeforeNewPins)
	t.Run("AppendsPinOrder", testAppendsPinOrder)
	t.Run("MovePin", testMovePin)
	t.Run("Window", testWindow)
	t.Run("RecentOrder", testRecentOrder)
	t.Run("AdjacentWrapsInSidebarOrder", testAdjacentWrapsInSidebarOrder)
	t.Run("ValidTitle", testValidTitle)
}

func testOrdersLegacyPinsBeforeNewPins(t *testing.T) {
	zero, one := 0, 1
	now := time.Now()
	got := ids(OrderPins([]Pin{
		{ID: "b", Order: &one},
		{ID: "a", Order: &zero},
		{ID: "old", LastRun: now.Add(-time.Hour)},
		{ID: "new", LastRun: now},
	}))
	if want := []string{"new", "old", "a", "b"}; !slices.Equal(got, want) {
		t.Errorf("got %v, want %v", got, want)
	}
}

func testAppendsPinOrder(t *testing.T) {
	zero, two := 0, 2
	if n := NextPinOrder(nil); n != 0 {
		t.Errorf("first pin: %d", n)
	}
	if n := NextPinOrder([]*int{&zero, nil, &two}); n != 3 {
		t.Errorf("next pin: %d", n)
	}
}

func testMovePin(t *testing.T) {
	list := []string{"a", "b", "c"}
	if got, ok := MovePin(list, "a", "c", true); !ok || !slices.Equal(got, []string{"b", "c", "a"}) {
		t.Errorf("after: %v %v", got, ok)
	}
	if got, ok := MovePin(list, "c", "a", false); !ok || !slices.Equal(got, []string{"c", "a", "b"}) {
		t.Errorf("before: %v %v", got, ok)
	}
	if _, ok := MovePin(list, "a", "b", false); ok {
		t.Error("a move to where it is changed the order")
	}
	if _, ok := MovePin(list, "a", "a", true); ok {
		t.Error("a drop on itself moved")
	}
}

func testWindow(t *testing.T) {
	for _, c := range []struct {
		n        int
		expanded bool
		shown    int
		more     bool
	}{{3, false, 3, false}, {5, false, 5, false}, {6, false, 5, true}, {6, true, 6, true}} {
		if shown, more := Window(c.n, c.expanded); shown != c.shown || more != c.more {
			t.Errorf("Window(%d, %v) = %d, %v", c.n, c.expanded, shown, more)
		}
	}
}

func testRecentOrder(t *testing.T) {
	got := RecentOrder([]Recent{{ID: "a", Created: 1}, {ID: "b", Created: 3, Archived: true}, {ID: "c", Created: 2}, {ID: "d", Created: 2}})
	var names []string
	for _, r := range got {
		names = append(names, r.ID)
	}
	if want := []string{"c", "d", "a"}; !slices.Equal(names, want) {
		t.Errorf("got %v, want %v", names, want)
	}
}

func testAdjacentWrapsInSidebarOrder(t *testing.T) {
	list := []string{"a", "b", "c"}
	for _, c := range []struct {
		sel     string
		forward bool
		want    string
	}{{"a", true, "b"}, {"c", true, "a"}, {"a", false, "c"}, {"", true, "a"}, {"", false, "c"}} {
		if got := Adjacent(list, c.sel, c.forward); got != c.want {
			t.Errorf("Adjacent(%q, %v) = %q, want %q", c.sel, c.forward, got, c.want)
		}
	}
	if Adjacent(nil, "a", true) != "" {
		t.Error("an empty list gave a conversation")
	}
}

func testValidTitle(t *testing.T) {
	if s, ok := ValidTitle("  Plan  "); !ok || s != "Plan" {
		t.Errorf("trim: %q %v", s, ok)
	}
	for _, bad := range []string{"", "   ", "a\nb"} {
		if _, ok := ValidTitle(bad); ok {
			t.Errorf("%q passed", bad)
		}
	}
}
