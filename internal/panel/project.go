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
}

// GitView is the Git component's state.
type GitView struct {
	change     string // the changed file shown, by path
	accepted   map[string]bool
	commit     string
	list, diff ui.ScrollState
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

// Show shows a changed file's hunks, from the top.
func (v *GitView) Show(path string) {
	if path != v.change {
		v.change, v.diff.X, v.diff.Y = path, 0, 0
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

// Of returns the project's state, the same object for every call.
func (ps *Projects) Of(id string) *Project {
	if p, ok := ps.of[id]; ok {
		return p
	}
	p := &Project{ID: id, Data: ps.data, Files: FileView{file: "sailry-go/README.md", folders: map[string]*bool{}},
		Git: GitView{accepted: map[string]bool{}}}
	if len(ps.data.Changes) > 0 {
		p.Git.change = ps.data.Changes[0].Path
	}
	ps.of[id] = p
	return p
}
