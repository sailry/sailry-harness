// Package panes is the split workspace's model: which sessions and
// terminals share the main column, and how (sailry-public
// apps/desktop/src/panes.rs, panes/*.rs, shell/splits.rs). A workspace
// group is a layout tree of splits, each along an axis with its
// children's shares, and leaves each holding one target. The tree
// follows the rules of the Kit dock Rust builds on (gpui-component
// crates/base/src/dock/layout/edit.rs and normalize.rs): a drop beside a
// pane splits that pane's slot, a split of another axis wraps the pane,
// and every edit normalizes, dropping empty children, collapsing a split
// left with one child and splicing a split into a parent of its axis.
//
// A target in no group stands alone, as Rust's standalone workspace of
// one pane; it holds no layout, so only groups of two or more panes are
// kept. Kit keeps sizes in pixels with unknown slots; here a split keeps
// shares of its length that sum to 1, which persist and resize without
// a measured window.
package panes

import "slices"

// Limit is how many panes one workspace holds (panes.rs LIMIT).
const Limit = 9

// Kind is what a pane shows.
type Kind string

const (
	Session  Kind = "session"
	Terminal Kind = "terminal"
)

// Target is a pane's content: a session by its ID, or a terminal by the
// ID of its tab (panes.rs Target).
type Target struct {
	Kind Kind   `json:"kind"`
	ID   string `json:"id"`
}

// Axis is a split's direction: Row lays its children left to right,
// Column top to bottom.
type Axis int

const (
	Row Axis = iota
	Column
)

// Placement is where a drop lands on a pane: beside it on a side, or in
// its place (Center), as Kit's Option<Placement> with None the centre.
type Placement int

const (
	Center Placement = iota
	Left
	Right
	Top
	Bottom
)

// axis is the axis a placement splits along.
func (p Placement) axis() Axis {
	if p == Top || p == Bottom {
		return Column
	}
	return Row
}

// Drag is the value dragged onto the workspace from a session row, a
// terminal tab or a pane's header (panes.rs Drag): the target and the
// title its drag preview shows.
type Drag struct {
	Target Target
	Title  string
}

// Node is a layout tree's node: a leaf holding a Target, or a split of
// two or more children along Axis, Sizes their shares of its length.
type Node struct {
	Target   Target    `json:"target,omitzero"`
	Axis     Axis      `json:"axis,omitempty"`
	Children []*Node   `json:"children,omitempty"`
	Sizes    []float32 `json:"sizes,omitempty"`
}

// Leaf reports whether n holds a target.
func (n *Node) Leaf() bool { return len(n.Children) == 0 }

// Targets are the leaves' targets in visual order.
func (n *Node) Targets() []Target {
	var out []Target
	n.walk(func(t Target) { out = append(out, t) })
	return out
}

func (n *Node) walk(fn func(Target)) {
	if n.Leaf() {
		fn(n.Target)
		return
	}
	for _, ch := range n.Children {
		ch.walk(fn)
	}
}

// find returns the leaf holding t, its parent and its index there; the
// parent is nil when n is the leaf.
func (n *Node) find(t Target) (leaf, parent *Node, ix int) {
	if n.Leaf() {
		if n.Target == t {
			return n, nil, 0
		}
		return nil, nil, 0
	}
	for i, ch := range n.Children {
		if ch.Leaf() && ch.Target == t {
			return ch, n, i
		}
		if leaf, parent, ix = ch.find(t); leaf != nil {
			return leaf, parent, ix
		}
	}
	return nil, nil, 0
}

// Workspace is every group of panes and the pane with the focus.
type Workspace struct {
	Groups []*Node `json:"groups,omitempty"`
	// Active is the focused pane, whose group the main column shows.
	Active Target `json:"-"`
}

// Group returns the index of the group holding t, or -1 when t stands
// alone.
func (w *Workspace) Group(t Target) int {
	for i, g := range w.Groups {
		if leaf, _, _ := g.find(t); leaf != nil {
			return i
		}
	}
	return -1
}

// Displayed is the group the main column shows, the active pane's, or
// nil when it stands alone.
func (w *Workspace) Displayed() *Node {
	if len(w.Groups) == 0 {
		return nil
	}
	if i := w.Group(w.Active); i >= 0 {
		return w.Groups[i]
	}
	return nil
}

// Members are the panes sharing t's workspace in visual order, t alone
// when it stands alone (shell ordered_members).
func (w *Workspace) Members(t Target) []Target {
	if i := w.Group(t); i >= 0 {
		return w.Groups[i].Targets()
	}
	return []Target{t}
}

// CanDrop reports whether t may land on dest at p (panes.rs can_drop): a
// split adds a pane, so a full workspace takes only its own panes and
// replacements.
func (w *Workspace) CanDrop(dest, t Target, p Placement) bool {
	if dest == t || dest == (Target{}) || t == (Target{}) {
		return false
	}
	g := w.Group(dest)
	if p == Center {
		// A replacement leaves t standing alone in dest's place when dest
		// stands alone, and only a session can show alone.
		return g >= 0 || t.Kind == Session
	}
	return g >= 0 && g == w.Group(t) || len(w.Members(dest)) < Limit
}

// Drop puts t on dest at p (panes.rs drop_to): beside dest on a side,
// splitting dest's slot; in dest's place at the centre, swapping the two
// within one group, or else leaving dest to stand alone. t leaves the
// group it was in and takes the focus. It reports whether anything
// changed.
func (w *Workspace) Drop(dest, t Target, p Placement) bool {
	if !w.CanDrop(dest, t, p) {
		return false
	}
	gd, gt := w.Group(dest), w.Group(t)
	if p == Center {
		switch {
		case gd >= 0 && gd == gt:
			a, _, _ := w.Groups[gd].find(dest)
			b, _, _ := w.Groups[gd].find(t)
			a.Target, b.Target = t, dest
		case gd >= 0:
			w.remove(t)
			leaf, _, _ := w.Groups[w.Group(dest)].find(dest)
			leaf.Target = t
		default:
			w.remove(t)
		}
		w.Active = t
		return true
	}
	w.remove(t)
	gd = w.Group(dest)
	if gd < 0 {
		w.Groups = append(w.Groups, &Node{Target: dest})
		gd = len(w.Groups) - 1
	}
	w.Groups[gd] = insert(w.Groups[gd], dest, t, p)
	w.Active = t
	return true
}

// Detach takes t out of its group to stand alone (panes.rs detach), the
// focus going to the pane of the group it was in that had it, or to the
// group's first (resources/close.rs close_pane).
func (w *Workspace) Detach(t Target) bool {
	g := w.Group(t)
	if g < 0 {
		return false
	}
	members := w.Groups[g].Targets()
	w.remove(t)
	if w.Active == t {
		for _, m := range members {
			if m != t {
				w.Active = m
				break
			}
		}
	}
	return true
}

// Prune drops the panes keep refuses, as of sessions and terminals no
// longer there.
func (w *Workspace) Prune(keep func(Target) bool) {
	for _, g := range slices.Clone(w.Groups) {
		for _, t := range g.Targets() {
			if !keep(t) {
				w.remove(t)
			}
		}
	}
}

// remove takes t out of its group, normalizing it and dissolving a group
// left with fewer than two panes.
func (w *Workspace) remove(t Target) {
	g := w.Group(t)
	if g < 0 {
		return
	}
	root := w.Groups[g]
	if _, parent, ix := root.find(t); parent != nil {
		parent.Children = slices.Delete(parent.Children, ix, ix+1)
		parent.Sizes = slices.Delete(parent.Sizes, ix, ix+1)
		rescale(parent.Sizes)
		root = normalize(root)
	}
	if root.Leaf() {
		w.Groups = slices.Delete(w.Groups, g, g+1)
		return
	}
	w.Groups[g] = root
}

// insert puts t in a new leaf beside dest at p, and returns the tree's
// root (edit.rs insert_beside): in a parent split along p's axis the new
// leaf takes half of dest's slot; otherwise dest is wrapped in a new
// split of that axis, half and half.
func insert(root *Node, dest, t Target, p Placement) *Node {
	leaf, parent, ix := root.find(dest)
	if leaf == nil {
		return root
	}
	at := ix + 1
	if p == Left || p == Top {
		at = ix
	}
	added := &Node{Target: t}
	if parent != nil && parent.Axis == p.axis() {
		half := parent.Sizes[ix] / 2
		parent.Sizes[ix] = half
		parent.Children = slices.Insert(parent.Children, at, added)
		parent.Sizes = slices.Insert(parent.Sizes, at, half)
		return normalize(root)
	}
	moved := &Node{Target: leaf.Target}
	children := []*Node{moved, added}
	if p == Left || p == Top {
		children = []*Node{added, moved}
	}
	// The leaf becomes the split in place, so its slot in the parent stays.
	*leaf = Node{Axis: p.axis(), Children: children, Sizes: []float32{0.5, 0.5}}
	return normalize(root)
}

// normalize applies the dock's rules (normalize.rs) to n and returns the
// node that replaces it: empty splits go, a split of one child becomes
// that child, and a child split along its parent's axis is spliced into
// the parent, its shares scaled to the slot it had.
func normalize(n *Node) *Node {
	if n.Leaf() {
		return n
	}
	var children []*Node
	var sizes []float32
	for i, ch := range n.Children {
		ch = normalize(ch)
		if !ch.Leaf() && ch.Axis == n.Axis {
			for j, inner := range ch.Children {
				children = append(children, inner)
				sizes = append(sizes, ch.Sizes[j]*n.Sizes[i])
			}
			continue
		}
		children = append(children, ch)
		sizes = append(sizes, n.Sizes[i])
	}
	n.Children, n.Sizes = children, sizes
	rescale(n.Sizes)
	if len(n.Children) == 1 {
		return n.Children[0]
	}
	return n
}

// rescale makes shares sum to 1, splitting evenly when they cannot.
func rescale(sizes []float32) {
	var total float32
	for _, s := range sizes {
		total += max(s, 0)
	}
	for i, s := range sizes {
		if total <= 0 {
			sizes[i] = 1 / float32(len(sizes))
		} else {
			sizes[i] = max(s, 0) / total
		}
	}
}

// Valid reports whether a group read back is a tree this package makes:
// splits of two or more children with a share each, leaves holding a
// target, no target twice, at most Limit panes.
func Valid(n *Node) bool {
	seen := map[Target]bool{}
	var ok func(*Node) bool
	ok = func(n *Node) bool {
		if n == nil {
			return false
		}
		if n.Leaf() {
			if n.Target == (Target{}) || seen[n.Target] {
				return false
			}
			seen[n.Target] = true
			return true
		}
		if len(n.Children) < 2 || len(n.Sizes) != len(n.Children) || n.Target != (Target{}) {
			return false
		}
		for _, ch := range n.Children {
			if !ok(ch) {
				return false
			}
		}
		return true
	}
	return ok(n) && !n.Leaf() && len(seen) <= Limit
}

// Restore takes groups read back from preferences: invalid trees go, as
// does a group sharing a target with an earlier one, and shares are made
// to sum to 1.
func (w *Workspace) Restore(groups []*Node) {
	w.Groups = nil
	for _, g := range groups {
		if !Valid(g) || slices.ContainsFunc(g.Targets(), func(t Target) bool { return w.Group(t) >= 0 }) {
			continue
		}
		g = normalize(g)
		w.Groups = append(w.Groups, g)
	}
}
