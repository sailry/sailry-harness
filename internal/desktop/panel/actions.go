package panel

import (
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/editor"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

// Actions connects existing workspace controls to the live controller. Preview
// leaves it nil and retains its isolated notice behavior.
type Actions struct {
	ReadDirectory    func(string)
	NextDirectory    func(string)
	RefreshDirectory func(string)
	PasteReady       func() bool
	ReadFile         func(string)
	OpenFile         func(string) bool
	ReadNotebook     func(string, uint64)
	SaveFile         func(*ui.Context, string, string)
	File             func(*ui.Context, FileAction)
	Git              func(*ui.Context, GitAction)
	ReadDiff         func(string, string)
	ReadCommit       func(string)
	GitHistory       func() // reads the commits after the ones listed
	ReadDocument     func(string)
	Hunk             func(*ui.Context, string, int, string)
	Restore          func(*ui.Context, string)
	Refresh          func()
	Search           func(SearchQuery, func(SearchResult))
	CancelSearch     func()
	OpenFileAt       func(string, int)
	Upload           func(string)
	Pending          func() []PendingOperation
	PendingGit       func() []PendingOperation
	// Loading reports whether the session's checkpoints (History) or
	// documents (Documents) are being read.
	Loading func(Kind) bool
	GitBusy func() bool // a Git action is under way
}
type SearchQuery struct {
	Query, Filter        string
	CaseSensitive, Regex bool
}
type SearchMatch struct {
	Path, Text string
	Line       int
}
type SearchResult struct {
	Matches         []SearchMatch
	Partial, Failed bool
}
type PendingOperation struct {
	Label string
	Run   func(*ui.Context)
}
type DirectoryState struct{ Loaded, Loading, Failed, Partial, Next bool }

func (pr *Project) SetDirectoryState(path string, state DirectoryState) {
	if pr.Files.directories == nil {
		pr.Files.directories = map[string]DirectoryState{}
	}
	pr.Files.directories[path] = state
}
func (pr *Project) PruneFileSelection() {
	for path := range pr.Files.sel.selected {
		if findFile(pr.Data.Files, path) == nil {
			delete(pr.Files.sel.selected, path)
		}
	}
	if findFile(pr.Data.Files, pr.Files.sel.current) == nil {
		pr.Files.sel.current = ""
	}
	if findFile(pr.Data.Files, pr.Files.sel.anchor) == nil {
		pr.Files.sel.anchor = ""
	}
}

type FileAction struct {
	Kind                               string
	Paths                              []string
	Directory, Destination, Name, Text string
}
type GitAction struct {
	Kind, Target, Secondary, Message, Remote, Scope string
	Paths                                           []string
	Amend, Signoff, SkipHooks, Rebase, Force        bool
	After                                           string
}
type Choice struct {
	Label, Detail string
	Run           func(*ui.Context)
}

func (pr *Project) Choose(title string, choices []Choice) {
	items := make([]action, 0, len(choices))
	for _, choice := range choices {
		items = append(items, action{label: choice.Label, detail: choice.Detail, run: choice.Run})
	}
	pr.Git.picker = picker{open: true, title: title, items: items}
}
func (pr *Project) SetGitTargets(stashes []string, tags map[string]string) {
	pr.Git.stashIDs = append([]string(nil), stashes...)
	pr.Git.tagIDs = tags
}
func (pr *Project) SetGitOperation(active bool) { pr.Git.operation = active }

func NewLiveProject(id string, data *viewmodel.Workspace, actions *Actions) *Project {
	return &Project{ID: id, Data: data, Actions: actions, Files: FileView{items: map[string]*fileItem{}, folders: map[string]*bool{}, sel: selection{selected: map[string]bool{}}, side: sideWidth}, Git: GitView{side: sideWidth, scope: "all", nav: defaultNav, expanded: map[string]*bool{}, folded: map[string]bool{}, after: "none"}}
}
func (pr *Project) fileAction(c *ui.Context, action FileAction) {
	if pr.Actions != nil && pr.Actions.File != nil {
		action.Paths = append([]string(nil), action.Paths...)
		pr.Actions.File(c, action)
		return
	}
	notice(c)
}
func (pr *Project) gitAction(c *ui.Context, action GitAction) {
	if pr.Actions != nil && pr.Actions.Git != nil {
		action.Paths = append([]string(nil), action.Paths...)
		pr.Actions.Git(c, action)
		return
	}
	notice(c)
}
func (pr *Project) refresh(c *ui.Context) {
	if pr.Actions != nil && pr.Actions.Refresh != nil {
		pr.Actions.Refresh()
		return
	}
	notice(c)
}
func (pr *Project) readFile(path string) {
	if pr.Actions != nil && pr.Actions.ReadFile != nil {
		pr.Actions.ReadFile(path)
	}
}
func (pr *Project) readDiff(path, scope string) {
	if pr.Actions != nil && pr.Actions.ReadDiff != nil {
		pr.Actions.ReadDiff(path, scope)
	}
}
func (pr *Project) SetFile(path, text string) {
	item := pr.Files.item(path)
	if !item.loaded || !item.dirty {
		item.base = text
		item.draft = strings.TrimRight(text, "\r\n")
		item.suffix = text[len(item.draft):]
		item.loaded = true
		item.dirty = false
	}
	if file := findFile(pr.Data.Files, path); file != nil {
		file.Text = text
	}
}
func (pr *Project) SavedFile(path, text string) {
	item := pr.Files.item(path)
	if item.draft+item.suffix != text {
		item.base = text
		item.dirty = true
		return
	}
	item.dirty = false
	item.base = text
	item.loaded = true
	if file := findFile(pr.Data.Files, path); file != nil {
		file.Text = text
	}
	if item.editor != nil {
		item.editor.Buffer.MarkSaved()
	}
}
func (pr *Project) CommitSaved(message string) {
	if pr.Git.commit == message {
		pr.Git.commit = ""
	}
	pr.Git.amend, pr.Git.signoff, pr.Git.skip, pr.Git.tall = false, false, false, false
	pr.Git.after, pr.Git.draft = "none", ""
}
func (pr *Project) SetFileWritable(path string, writable bool) {
	pr.Files.item(path).writable = writable
}
func (pr *Project) FileDraft(path string) (string, bool) {
	item := pr.Files.item(path)
	return item.draft + item.suffix, item.dirty
}
func (pr *Project) FileOpen(path string) { pr.Files.Show(path); pr.readFile(path) }
func (pr *Project) DiffOpen(path, scope string) {
	pr.Git.Show(path)
	pr.Git.scope = scope
	pr.readDiff(path, scope)
}
func (pr *Project) FileChanged(path, text string) {
	item := pr.Files.item(path)
	item.draft = text
	item.dirty = text+item.suffix != item.base
}
func (pr *Project) FileOpenAt(path string, line int) {
	pr.FileOpen(path)
	item := pr.Files.item(path)
	item.pendingLine = max(1, line)
	item.source = 1
}
func (pr *Project) bindEditor(path string, state *editor.State) {
	item := pr.Files.item(path)
	item.editor = state
	if item.loaded && item.pendingLine > 0 {
		line := item.pendingLine - 1
		state.GoTo(editor.Pos{Line: line})
		item.text.Y = float32(state.LineTop(line))
		item.pendingLine = 0
	}
}
