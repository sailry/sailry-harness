package panel

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/preview"
)

// Project is a project's Files and Git state. Its two hosts, the
// independent page in the main column and the tab in the right panel,
// both draw from and write to it, so a file chosen on the page is the one
// the tab shows, and the reverse (docs/panel.md, Components and hosts).
type Project struct {
	ID    string
	Data  *preview.Workspace
	Files FileView
	Git   GitView
}

// FileView is the Files component's state.
type FileView struct {
	file       string // the file shown, by path
	folders    map[string]*bool
	source     int // 0 the document, 1 its source, for markdown
	tree, text ui.ScrollState
	side       float32 // the tree's width
}

// GitView is the Git component's state.
type GitView struct {
	change string // the changed file shown, by path
	commit string
	list   ui.ScrollState
	side   float32 // the list's width
}

// File is the path of the file shown.
func (v *FileView) File() string { return v.file }

// Show shows a file, from the top.
func (v *FileView) Show(path string) {
	if path != v.file {
		v.file, v.text.X, v.text.Y = path, 0, 0
	}
}

// Change is the path of the changed file shown.
func (v *GitView) Change() string { return v.change }

// Show shows a changed file's diff, from the top: each file's viewer is
// its own.
func (v *GitView) Show(path string) { v.change = path }

// Projects keeps each project's state, made on first use and kept while
// the app runs, so leaving a project and coming back finds it as it was.
type Projects struct {
	data *preview.Workspace
	of   map[string]*Project
}

func NewProjects(w *preview.Workspace) *Projects {
	return &Projects{data: w, of: map[string]*Project{}}
}

// Of returns the project's state, the same object for every call.
func (ps *Projects) Of(id string) *Project {
	if p, ok := ps.of[id]; ok {
		return p
	}
	p := &Project{ID: id, Data: ps.data, Files: FileView{file: "sailry-go/README.md", folders: map[string]*bool{}, side: sideWidth},
		Git: GitView{side: sideWidth}}
	if len(ps.data.Changes) > 0 {
		p.Git.change = ps.data.Changes[0].Path
	}
	ps.of[id] = p
	return p
}

// The side list's width (resources/split.rs): 220 at first, dragged
// between sideMin and sideMax, the main area keeping sideMin.
const (
	sideWidth = 220
	sideMin   = 180
	sideMax   = 480
)

// split is resources/split.rs Split: the main area, then the side list at
// its own width past MyGo's divider. MyGo's Split sizes its first pane,
// so the list's width is turned into the main area's from the width the
// last frame laid out, and back after a drag, so the list keeps its
// width as the panel resizes.
func split(c *ui.Context, key string, side *float32, main, list func()) {
	box := ui.Column(c.Key(key)).Grow(1).MinHeight(0).MinWidth(0)
	w := box.LayoutBounds().W
	clamp := func(v float32) float32 { return min(max(v, sideMin), max(w-sideMin, sideMin), sideMax) }
	box.Children(func() {
		size := w - clamp(*side)
		if w <= 0 {
			size = sideMin // the first frame: laid out again at once
		}
		sp := ui.Split(c, &size, func() {
			ui.Column(c.Key("main")).Grow(1).MinWidth(0).MinHeight(0).Children(main)
		}, func() {
			ui.Column(c.Key("side")).Grow(1).MinWidth(0).MinHeight(0).Children(list)
		}).Grow(1).MinHeight(0)
		if sp.Changed() && w > 0 {
			*side = clamp(w - size)
		}
	})
}
