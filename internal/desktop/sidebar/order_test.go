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
	t.Run("Sorted", testSorted)
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

// The fixture is in creation order, newest first, and tells the three
// orders apart: the newest never ran, and names, runs and creation disagree.
func testSorted(t *testing.T) {
	at := func(minute int) time.Time { return time.Date(2026, 11, 11, 9, minute, 0, 0, time.UTC) }
	list := []Entry{
		{ID: "new", Name: ""},
		{ID: "c", Name: "cedar", Updated: at(10)},
		{ID: "a", Name: "Alder", Updated: at(30)},
		{ID: "b", Name: "birch", Updated: at(20)},
		{ID: "b2", Name: "Birch", Updated: at(20)},
	}
	for _, c := range []struct {
		by   Sort
		want []string
	}{
		{SortCreated, []string{"new", "c", "a", "b", "b2"}},
		{SortUpdated, []string{"new", "a", "b", "b2", "c"}},
		{SortName, []string{"new", "a", "b", "b2", "c"}},
		{Sort(9), []string{"new", "c", "a", "b", "b2"}},
	} {
		var got []string
		for _, e := range Sorted(list, c.by) {
			got = append(got, e.ID)
		}
		if !slices.Equal(got, c.want) {
			t.Errorf("Sorted by %d = %v, want %v", c.by, got, c.want)
		}
	}
	if list[0].ID != "new" || list[4].ID != "b2" {
		t.Error("Sorted changed its input")
	}
	// The preference's values: Rust's created and updated, and name; a
	// profile without one, or with one from elsewhere, is updated.
	for value, want := range map[string]Sort{"created": SortCreated, "updated": SortUpdated, "name": SortName, "": SortUpdated, "manual": SortUpdated} {
		if got := SortOf(value); got != want {
			t.Errorf("SortOf(%q) = %d, want %d", value, got, want)
		}
	}
	for by, want := range map[Sort]string{SortCreated: "created", SortUpdated: "updated", SortName: "name"} {
		if got := by.Preference(); got != want {
			t.Errorf("Sort %d is %q, want %q", by, got, want)
		}
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
