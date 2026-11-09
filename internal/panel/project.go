package panel

import (
	"slices"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/preview"
	"github.com/sailry/sailry-go/internal/theme"
)

// Project is a project's Files and Git state. Its two hosts, the
// independent page in the main column and the right panel's tabs, both
// draw from and write to it: the files and diffs open are the same items
// in the page's strip and the panel's, each with its own state, and the
// item shown last is the one either host shows first (docs/panel.md,
// Components and hosts).
type Project struct {
	ID    string
	Data  *preview.Workspace
	Files FileView
	Git   GitView
}

// FileView is the Files component's state: the files open, each with its
// own view, and the tree's.
type FileView struct {
	file    string   // the file shown last, by path
	open    []string // the files open, in order
	items   map[string]*fileItem
	folders map[string]*bool
	tree    ui.ScrollState
	side    float32 // the tree's width
}

// fileItem is an open file's own view.
type fileItem struct {
	source int // 0 the document, 1 its source, for markdown
	text   ui.ScrollState
}

// GitView is the Git component's state: the diffs open (each viewer
// keeps its own scroll), the list's and the commit draft.
type GitView struct {
	change string   // the changed file shown last, by path
	open   []string // the diffs open, in order
	commit string
	list   ui.ScrollState
	side   float32 // the list's width
}

// File is the path of the file shown.
func (v *FileView) File() string { return v.file }

// Open are the files open, in order.
func (v *FileView) Open() []string { return v.open }

// Show opens a file, or focuses it when open.
func (v *FileView) Show(path string) {
	if path == "" {
		return
	}
	v.file = path
	if !slices.Contains(v.open, path) {
		v.open = append(v.open, path)
	}
}

// Close closes a file; the file shown moves to the one after it, else
// before it.
func (v *FileView) Close(path string) {
	v.open, v.file = closeItem(v.open, path, v.file)
	delete(v.items, path)
}

// item is an open file's view, made on first use.
func (v *FileView) item(path string) *fileItem {
	it := v.items[path]
	if it == nil {
		it = &fileItem{}
		v.items[path] = it
	}
	return it
}

// Change is the path of the changed file shown.
func (v *GitView) Change() string { return v.change }

// Open are the diffs open, in order.
func (v *GitView) Open() []string { return v.open }

// Show opens a changed file's diff, or focuses it when open.
func (v *GitView) Show(path string) {
	if path == "" {
		return
	}
	v.change = path
	if !slices.Contains(v.open, path) {
		v.open = append(v.open, path)
	}
}

// Close closes a diff, as FileView.Close.
func (v *GitView) Close(path string) { v.open, v.change = closeItem(v.open, path, v.change) }

// closeItem takes path out of open; when it was shown, the one after it
// is, else the one before, else none.
func closeItem(open []string, path, shown string) ([]string, string) {
	i := slices.Index(open, path)
	if i < 0 {
		return open, shown
	}
	open = slices.Delete(slices.Clone(open), i, i+1)
	if shown == path {
		shown = ""
		if len(open) > 0 {
			shown = open[min(i, len(open)-1)]
		}
	}
	return open, shown
}

// view is the state of k's items.
func (pr *Project) items(k Kind) []string {
	if k == Files {
		return pr.Files.open
	}
	return pr.Git.open
}

func (pr *Project) shown(k Kind) string {
	if k == Files {
		return pr.Files.file
	}
	return pr.Git.change
}

func (pr *Project) show(k Kind, path string) {
	if k == Files {
		pr.Files.Show(path)
	} else {
		pr.Git.Show(path)
	}
}

func (pr *Project) close(k Kind, path string) {
	if k == Files {
		pr.Files.Close(path)
	} else {
		pr.Git.Close(path)
	}
}

// Projects keeps each project's state, made on first use and kept while
// the app runs, so leaving a project and coming back finds it as it was.
type Projects struct {
	data *preview.Workspace
	of   map[string]*Project
}

func NewProjects(w *preview.Workspace) *Projects {
	return &Projects{data: w, of: map[string]*Project{}}
}

// Of returns the project's state, the same object for every call. Nothing
// is open at first; the file and the change a host shows first are the
// README and the first change.
func (ps *Projects) Of(id string) *Project {
	if p, ok := ps.of[id]; ok {
		return p
	}
	p := &Project{ID: id, Data: ps.data, Files: FileView{file: "sailry-go/README.md", items: map[string]*fileItem{},
		folders: map[string]*bool{}, side: sideWidth}, Git: GitView{side: sideWidth}}
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
	handle    = 6
)

// split is resources/split.rs Split: the main area, then the side list at
// its own width past a drag handle. Custom, not MyGo's Split: that sizes
// its first pane, so keeping the list's width meant converting it from
// the last frame's layout, and a first frame (a page switched to, a tab
// chosen, the panel opening) drew the main area at its minimum. Here the
// list is a fixed-width column from the first frame; the main area takes
// the rest and the motion of the panel only moves the main area. Only a
// drag reads the layout, to keep the main area sideMin wide.
func split(c *ui.Context, key string, side *float32, main, list func()) {
	box := ui.Row(c.Key(key)).Grow(1).MinHeight(0).MinWidth(0).AlignItems(ui.Stretch).Clip()
	w := box.LayoutBounds().W
	box.Children(func() {
		ui.Column(c.Key("main")).Grow(1).Shrink(1).MinWidth(0).MinHeight(0).Children(main)
		divider(c, side, w)
		ui.Column(c.Key("side")).Width(*side).Shrink(0).MinHeight(0).Children(list)
	})
}

// divider is the split's 1 DIP line under a 6 DIP drag handle: dragging
// left widens the list.
func divider(c *ui.Context, side *float32, w float32) {
	p := theme.Of(c)
	h := ui.Box(c.Key("divider")).Width(handle).Margin(0, -handle/2).ZIndex(1).Cursor(ui.CursorResizeEW).Shrink(0).
		Row().Justify(ui.Center).Children(func() {
		ui.Box(c).Width(1).FillHeight().Background(p.Border)
	})
	if h.Hovered() || h.Pressed() {
		h.Background(p.Ring.Alpha(0.5))
	}
	if dx, _, ok := h.Dragged(); ok {
		limit := float32(sideMax)
		if w > 0 {
			limit = min(limit, max(w-sideMin, sideMin))
		}
		*side = min(max(*side-dx, sideMin), limit)
	}
}
