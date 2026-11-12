package panel

import (
	"slices"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/editor"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
)

// Project is a project's Files and Git state. Its two hosts, the
// independent page in the main column and the right panel's tabs, both
// draw from and write to it: the files and diffs open are the same items
// in the page's strip and the panel's, each with its own state, and the
// item shown last is the one either host shows first.
type Project struct {
	ID       string
	Actions  *Actions
	prompt   prompt
	closing  *editorCloseRequest
	conflict *editorConflict
	Data     *preview.Workspace
	Files    FileView
	Git      GitView
	// reveal shows a file of the tree in the host's Files, as the host
	// showing the project last sets it.
	reveal func(path string)
	// stacked and narrow are the layout of the host building the project
	// (pair): its list above its document, and a document too narrow for a
	// diff side by side.
	stacked, narrow bool
}

// FileView is the Files component's state: the files open, each with its
// own view, and the tree's: its open folders and its selection.
type FileView struct {
	file    string   // the file shown last, by path
	open    []string // the files open, in order
	items   map[string]*fileItem
	folders map[string]*bool
	sel     selection
	// focus is the row to take the keyboard focus while focusing is set.
	focus       string
	focusing    bool
	tree        ui.ScrollState
	side        float32 // the tree's width
	search      fileSearch
	searchOpen  bool
	directories map[string]DirectoryState
	// sampleClip is what the demo's tree copied or cut, in memory.
	sampleClip []string
	sampleCut  bool
}

// fileItem is an open file's own view.
type fileItem struct {
	base, draft, suffix string
	loaded, dirty       bool
	writable            bool
	editor              *editor.State
	document            *documentEditor
	pendingLine         int
	source              int // 0 the document, 1 its source, for markdown
	text                ui.ScrollState
}

// GitView is the Git component's state: the diffs open (each viewer
// keeps its own scroll), the details' view (repository.js: the list's
// layout, its rows open and its cursor, Changes or History, the commit
// draft and options) and the picker or confirmation showing.
type GitView struct {
	// sampleStashes are the changes the demo's stashes put away, beside the
	// repository's messages, and sampleCommits counts the commits it made.
	sampleStashes [][]preview.FileChange
	sampleCommits int
	change        string   // the changed file shown last, by path, or allChanges
	open          []string // the diffs open, in order
	scope         string   // the scope the diff shown opened in: all, staged, unstaged
	nav           changeNav
	expanded      map[string]*bool // the list's groups and folders, by row
	current       string           // the row of the list's cursor
	view          int              // 0 Changes, 1 History
	commitID      string           // the commit chosen in the history
	folded        map[string]bool  // All changes' files closed
	commit        string           // the commit message
	draft         string           // the message before amending
	tall          bool             // the message box expanded
	amend         bool
	signoff       bool
	skip          bool   // skip hooks
	after         string // what the commit does after: none, push, sync
	chord         bool   // Ctrl+G pressed, its second key to come
	second        bool   // the key after Ctrl+G came, for this frame to take
	picker        picker
	confirm       confirm
	stashIDs      []string
	tagIDs        map[string]string
	operation     bool
	live          GitState
	log           GitHistoryState
	switchPending []PendingOperation
	documents     map[string]*gitDocument
	diffStates    map[string]GitDiffState
	list          ui.ScrollState
	history       ui.ScrollState
	side          float32 // the list's width
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
func (v *GitView) Scope() string  { return v.scope }

// Open are the diffs open, in order.
func (v *GitView) Open() []string { return v.open }

// Show opens a changed file's diff, or focuses it when open.
func (v *GitView) Show(path string) {
	if path == "" {
		return
	}
	if path != v.change {
		v.scope = "all"
	}
	v.change = path
	if !slices.Contains(v.open, path) {
		v.open = append(v.open, path)
	}
}

// First names the change a host shows first while none is shown, as a new
// project's first change: its diff opens when a host shows it, not before.
func (v *GitView) First(path string) {
	if v.change == "" {
		v.change, v.scope = path, "all"
	}
}

// Close closes a diff, as FileView.Close.
func (v *GitView) Close(path string) {
	v.open, v.change = closeItem(v.open, path, v.change)
	delete(v.documents, path)
}

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
		if pr.requestEditorClose(path, func() { pr.Files.Close(path) }) {
			return
		}
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
		folders: map[string]*bool{}, sel: selection{selected: map[string]bool{}}, side: sideWidth},
		Git: GitView{side: sideWidth, scope: "all", nav: defaultNav, expanded: map[string]*bool{}, folded: map[string]bool{}, after: "none"}}
	if len(ps.data.Changes) > 0 {
		p.Git.change = ps.data.Changes[0].Path
	}
	ps.of[id] = p
	return p
}
