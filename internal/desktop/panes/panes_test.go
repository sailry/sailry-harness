package panes

import (
	"encoding/json"
	"fmt"
	"math"
	"slices"
	"strings"
	"testing"
)

func s(id string) Target    { return Target{Session, id} }
func term(id string) Target { return Target{Terminal, id} }

// shape writes a tree as row(a,b) and column(a,b), with shares.
func shape(n *Node) string {
	if n.Leaf() {
		return n.Target.ID
	}
	name := "row"
	if n.Axis == Column {
		name = "col"
	}
	var parts []string
	for i, ch := range n.Children {
		parts = append(parts, fmt.Sprintf("%s:%.2f", shape(ch), n.Sizes[i]))
	}
	return name + "(" + strings.Join(parts, " ") + ")"
}

// Drop splits beside a pane or wraps it, moves a pane within its group or to
// another, and at the center replaces or swaps.
func TestDrop(t *testing.T) {
	t.Run("SplitInsertsBesideAndWraps", func(t *testing.T) {
		var w Workspace
		if !w.Drop(s("a"), s("b"), Right) {
			t.Fatal("drop right")
		}
		if got := shape(w.Groups[0]); got != "row(a:0.50 b:0.50)" {
			t.Fatalf("right: %s", got)
		}
		if w.Active != s("b") {
			t.Fatalf("focus: %v", w.Active)
		}
		// Beside a pane of a row, a side drop splits that pane's slot.
		w.Drop(s("a"), s("c"), Left)
		if got := shape(w.Groups[0]); got != "row(c:0.25 a:0.25 b:0.50)" {
			t.Fatalf("left: %s", got)
		}
		// Across the row's axis the pane is wrapped in a column.
		w.Drop(s("b"), term("1"), Bottom)
		if got := shape(w.Groups[0]); got != "row(c:0.25 a:0.25 col(b:0.50 1:0.50):0.50)" {
			t.Fatalf("bottom: %s", got)
		}
		w.Drop(s("b"), s("d"), Top)
		if got := shape(w.Groups[0]); got != "row(c:0.25 a:0.25 col(d:0.25 b:0.25 1:0.50):0.50)" {
			t.Fatalf("top: %s", got)
		}
		if got := w.Members(s("a")); !slices.Equal(got, []Target{s("c"), s("a"), s("d"), s("b"), term("1")}) {
			t.Fatalf("members: %v", got)
		}
	})
	t.Run("MoveWithinAGroup", func(t *testing.T) {
		var w Workspace
		w.Drop(s("a"), s("b"), Right)
		w.Drop(s("b"), s("c"), Right)
		// Moving a to the bottom of c takes it out of the row first.
		w.Drop(s("c"), s("a"), Bottom)
		if got := shape(w.Groups[0]); got != "row(b:0.50 col(c:0.50 a:0.50):0.50)" {
			t.Fatalf("move: %s", got)
		}
		// Two panes of one group: dropping one on the other's side keeps the group.
		var v Workspace
		v.Drop(s("a"), s("b"), Right)
		v.Drop(s("b"), s("a"), Right)
		if len(v.Groups) != 1 || shape(v.Groups[0]) != "row(b:0.50 a:0.50)" {
			t.Fatalf("swap sides: %v", v.Groups)
		}
	})
	t.Run("CenterReplacesOrSwaps", func(t *testing.T) {
		var w Workspace
		w.Drop(s("a"), s("b"), Right)
		w.Drop(s("b"), s("c"), Bottom)
		// Within a group the two trade places.
		w.Drop(s("a"), s("c"), Center)
		if got := shape(w.Groups[0]); got != "row(c:0.50 col(b:0.50 a:0.50):0.50)" {
			t.Fatalf("swap: %s", got)
		}
		// From outside, the dropped pane takes the place, the other stands alone.
		w.Drop(s("b"), s("x"), Center)
		if got := shape(w.Groups[0]); got != "row(c:0.50 col(x:0.50 a:0.50):0.50)" {
			t.Fatalf("replace: %s", got)
		}
		if w.Group(s("b")) != -1 || w.Active != s("x") {
			t.Fatalf("displaced: %d %v", w.Group(s("b")), w.Active)
		}
		// A terminal cannot replace a pane standing alone: it would show alone.
		if w.CanDrop(s("b"), term("1"), Center) {
			t.Fatal("terminal alone")
		}
		if w.Drop(s("a"), s("a"), Right) {
			t.Fatal("onto itself")
		}
	})
	t.Run("MovingBetweenGroups", func(t *testing.T) {
		var w Workspace
		w.Drop(s("a"), s("b"), Right)
		w.Drop(s("c"), s("d"), Right)
		// b leaves its group, which dissolves, for the other one.
		w.Drop(s("c"), s("b"), Bottom)
		if len(w.Groups) != 1 || shape(w.Groups[0]) != "row(col(c:0.50 b:0.50):0.50 d:0.50)" {
			t.Fatalf("moved: %d %s", len(w.Groups), shape(w.Groups[0]))
		}
		if w.Group(s("a")) != -1 {
			t.Fatal("a stands alone")
		}
	})
}

func TestDetachCollapsesAndNormalizes(t *testing.T) {
	var w Workspace
	w.Drop(s("a"), s("b"), Right)
	w.Drop(s("b"), s("c"), Bottom)
	w.Drop(s("c"), s("d"), Right)
	if got := shape(w.Groups[0]); got != "row(a:0.50 col(b:0.50 row(c:0.50 d:0.50):0.50):0.50)" {
		t.Fatalf("built: %s", got)
	}
	// b goes: its column keeps one child, the row(c d), which is spliced
	// into the outer row with its shares scaled to the slot.
	w.Active = s("b")
	w.Detach(s("b"))
	if got := shape(w.Groups[0]); got != "row(a:0.50 c:0.25 d:0.25)" {
		t.Fatalf("detach: %s", got)
	}
	if w.Active != s("a") {
		t.Fatalf("focus after: %v", w.Active)
	}
	w.Detach(s("c"))
	if got := shape(w.Groups[0]); got != "row(a:0.67 d:0.33)" {
		t.Fatalf("rescaled: %s", got)
	}
	// One pane left stands alone, and the group is gone.
	w.Detach(s("d"))
	if len(w.Groups) != 0 || w.Displayed() != nil {
		t.Fatalf("dissolved: %v", w.Groups)
	}
	if w.Detach(s("a")) {
		t.Fatal("a pane alone has nothing to leave")
	}
}

func TestLimit(t *testing.T) {
	var w Workspace
	for i := 1; i < Limit; i++ {
		if !w.Drop(s("0"), s(fmt.Sprint(i)), Right) {
			t.Fatalf("pane %d", i)
		}
	}
	if n := len(w.Members(s("0"))); n != Limit {
		t.Fatalf("%d panes", n)
	}
	if w.CanDrop(s("0"), s("x"), Right) || w.Drop(s("0"), s("x"), Bottom) {
		t.Fatal("a tenth pane")
	}
	// A full workspace still rearranges its own panes and takes replacements.
	if !w.Drop(s("0"), s("5"), Bottom) || !w.Drop(s("1"), s("x"), Center) {
		t.Fatal("rearrange in a full workspace")
	}
	if n := len(w.Members(s("0"))); n != Limit {
		t.Fatalf("%d panes after", n)
	}
	var sum float64
	for _, v := range w.Groups[0].Sizes {
		sum += float64(v)
	}
	if math.Abs(sum-1) > 1e-5 {
		t.Fatalf("shares sum %v", sum)
	}
}

// Groups read back from preferences: the round trip, the trees refused, and
// the panes that are gone or away.
func TestRestore(t *testing.T) {
	t.Run("RoundTrip", func(t *testing.T) {
		var w Workspace
		w.Drop(s("a"), s("b"), Right)
		w.Drop(s("b"), term("2"), Bottom)
		w.Drop(s("c"), s("d"), Top)
		w.Groups[0].Sizes = []float32{0.3, 0.7}
		data, err := json.Marshal(w)
		if err != nil {
			t.Fatal(err)
		}
		var back Workspace
		if err := json.Unmarshal(data, &back); err != nil {
			t.Fatal(err)
		}
		var r Workspace
		r.Restore(back.Groups)
		if len(r.Groups) != 2 || shape(r.Groups[0]) != shape(w.Groups[0]) || shape(r.Groups[1]) != shape(w.Groups[1]) {
			t.Fatalf("round trip: %s", data)
		}
		// Broken or overlapping groups read back are dropped.
		bad := []*Node{
			{Axis: Row, Children: []*Node{{Target: s("a")}}, Sizes: []float32{1}},
			{Axis: Row, Children: []*Node{{Target: s("a")}, {Target: s("a")}}, Sizes: []float32{1, 1}},
			{Axis: Row, Children: []*Node{{Target: s("a")}, {}}, Sizes: []float32{1, 1}},
			{Axis: Row, Children: []*Node{{Target: s("a")}, {Target: s("b")}}, Sizes: []float32{2, 2}},
			{Axis: Column, Children: []*Node{{Target: s("b")}, {Target: s("c")}}, Sizes: []float32{1, 1}},
			{Target: s("z")},
		}
		r.Restore(bad)
		if len(r.Groups) != 1 || shape(r.Groups[0]) != "row(a:0.50 b:0.50)" {
			t.Fatalf("restore: %d", len(r.Groups))
		}
	})
	t.Run("Prune", func(t *testing.T) {
		var w Workspace
		w.Drop(s("a"), term("1"), Right)
		w.Drop(s("a"), s("b"), Bottom)
		w.Prune(func(t Target) bool { return t.Kind == Session })
		if got := shape(w.Groups[0]); got != "col(a:0.50 b:0.50)" {
			t.Fatalf("pruned: %s", got)
		}
	})
	// Follow holds the groups while a pane is away: it comes back to its
	// place and share, the held groups are the ones saved, and any other
	// change of the layout is kept instead.
	t.Run("Follow", func(t *testing.T) {
		var w Workspace
		w.Drop(s("a"), s("b"), Right)
		w.Drop(s("b"), s("c"), Bottom)
		w.Drop(s("x"), s("y"), Right)
		w.Groups[0].Sizes = []float32{0.3, 0.7}
		full := []string{shape(w.Groups[0]), shape(w.Groups[1])}
		away := map[Target]bool{}
		keep := func(t Target) bool { return !away[t] }
		w.Follow(keep)
		if len(w.Groups) != 2 || shape(w.Groups[0]) != full[0] || len(w.Saved()) != 2 {
			t.Fatalf("nothing away: %s", shape(w.Groups[0]))
		}
		away[s("c")], away[s("y")] = true, true
		w.Follow(keep)
		w.Follow(keep)
		if len(w.Groups) != 1 || shape(w.Groups[0]) != "row(a:0.30 b:0.70)" {
			t.Fatalf("away: %d groups, %s", len(w.Groups), shape(w.Groups[0]))
		}
		if saved := w.Saved(); len(saved) != 2 || shape(saved[0]) != full[0] || shape(saved[1]) != full[1] {
			t.Fatalf("saved while away: %d groups", len(saved))
		}
		delete(away, s("y"))
		w.Follow(keep)
		if len(w.Groups) != 2 || shape(w.Groups[1]) != full[1] || shape(w.Groups[0]) != "row(a:0.30 b:0.70)" {
			t.Fatalf("one back: %d groups", len(w.Groups))
		}
		delete(away, s("c"))
		w.Follow(keep)
		if len(w.Groups) != 2 || shape(w.Groups[0]) != full[0] || shape(w.Groups[1]) != full[1] {
			t.Fatalf("all back: %s", shape(w.Groups[0]))
		}
		// A share changed while a pane is away makes the layout the user's.
		away[s("c")] = true
		w.Follow(keep)
		w.Groups[0].Sizes[0], w.Groups[0].Sizes[1] = 0.6, 0.4
		if saved := w.Saved(); len(saved) != 2 || shape(saved[0]) != "row(a:0.60 b:0.40)" {
			t.Fatalf("saved after a change: %s", shape(saved[0]))
		}
		delete(away, s("c"))
		w.Follow(keep)
		if shape(w.Groups[0]) != "row(a:0.60 b:0.40)" || w.Group(s("c")) != -1 {
			t.Fatalf("a changed layout was replaced: %s", shape(w.Groups[0]))
		}
		// Restore starts over: nothing is held from before.
		away[s("a")] = true
		w.Follow(keep)
		w.Restore([]*Node{{Axis: Row, Children: []*Node{{Target: s("p")}, {Target: s("q")}}, Sizes: []float32{1, 1}}})
		w.Follow(keep)
		if saved := w.Saved(); len(saved) != 1 || shape(saved[0]) != "row(p:0.50 q:0.50)" {
			t.Fatalf("restored over held groups: %d", len(saved))
		}
	})
}

// Edits apply while the panes build, so a tree read before one keeps its
// shape: the build walking it reaches each target once.
func TestEditsLeaveAHeldTree(t *testing.T) {
	edits := []func(*Workspace){
		func(w *Workspace) { w.Drop(s("a"), s("c"), Center) },
		func(w *Workspace) { w.Drop(s("a"), s("c"), Left) },
		func(w *Workspace) { w.Drop(s("b"), s("d"), Bottom) },
		func(w *Workspace) { w.Drop(s("b"), s("e"), Center) },
		func(w *Workspace) { w.Detach(s("a")) },
		func(w *Workspace) { w.Prune(func(t Target) bool { return t != s("b") }) },
	}
	for i, edit := range edits {
		var w Workspace
		w.Drop(s("a"), s("b"), Right)
		w.Drop(s("b"), s("c"), Right)
		held := w.Groups[0]
		before := shape(held)
		edit(&w)
		if got := shape(held); got != before {
			t.Errorf("edit %d changed the held tree: %s to %s", i, before, got)
		}
	}
}
