package preview

import (
	"fmt"
	"strings"
	"time"
)

// Host is a computer running a Node: this one and a paired one
// (local_host_name, remote_host_name in the reference's preview).
type Host struct {
	ID     string
	Name   string // an i18n key for the preview hosts
	Local  bool
	Update bool // an update is available
}

// Worktree is a checkout of a project: the main one, or one made for a
// session.
type Worktree struct {
	Branch string
	Path   string
	Main   bool
}

// NoticeKind is what a notice tells (activity/notifications.rs).
type NoticeKind int

const (
	NoticeCompleted NoticeKind = iota
	NoticeFailed
	NoticeApproval
	NoticeInput
	NoticeInfo
)

// Notice is an entry of the activity centre.
type Notice struct {
	ID      string
	Kind    NoticeKind
	Title   string
	Message string
	Session string // the conversation it opens, "" for none
	Link    string // a knowledge source it opens, "" for none
	Host    string
	Time    time.Time
	Read    bool
}

// FileNode is a file or a folder of the project tree in the right panel.
type FileNode struct {
	Name     string
	Path     string
	Children []*FileNode // nil for a file
	Text     string
}

// LineKind is a diff line's side.
type LineKind int

const (
	Context LineKind = iota
	Added
	Removed
)

// DiffLine is a line of a hunk, with its numbers on either side (0 where
// it has none).
type DiffLine struct {
	Kind     LineKind
	Old, New int
	Text     string
}

// Hunk is a block of a file's changes.
type Hunk struct {
	Header string
	Lines  []DiffLine
}

// FileChange is a changed file of the working tree.
type FileChange struct {
	Path           string
	Status         string // M, A, D
	Added, Removed int
	Hunks          []Hunk
}

// Checkpoint is the files a turn changed, which undo restores.
type Checkpoint struct {
	ID             string
	Turn           string // the user's message of the turn
	Time           time.Time
	Files          []string
	Added, Removed int
}

// Document is a document of the session, as a plan or a report.
type Document struct {
	ID    string
	Title string
	Path  string
	Text  string
}

// Citation is a knowledge source a reply cited, in pages
// (shell/knowledge/viewer.rs).
type Citation struct {
	Link       string
	Title      string
	Host       string
	Path       string
	Start, End int // lines
	Pages      []string
}

// Workspace is the preview's hosts, notices and the right panel's data.
type Workspace struct {
	Hosts       []Host
	Notices     []*Notice
	Files       *FileNode
	Changes     []FileChange
	Checkpoints []Checkpoint
	Documents   []Document
	Citations   []Citation
}

// files of the panel's tree: the reference's two (preview.rs FILES) and a
// few more, so the tree has depth.
func files() *FileNode {
	f := func(path, text string) *FileNode {
		return &FileNode{Name: path[strings.LastIndex(path, "/")+1:], Path: path, Text: text}
	}
	dir := func(path string, kids ...*FileNode) *FileNode {
		return &FileNode{Name: path[strings.LastIndex(path, "/")+1:], Path: path, Children: kids}
	}
	return dir("sailry-go",
		dir("sailry-go/docs",
			f("sailry-go/docs/example.md", "# Desktop layout\n\nThe left column keeps the host and project tree, the middle displays resources, and the right opens as needed.\n\nNarrow windows collapse the right column first; manual choices are kept when width returns."),
		),
		dir("sailry-go/internal",
			dir("sailry-go/internal/shell",
				f("sailry-go/internal/shell/shell.go", "package shell\n\n// App is the window's state.\ntype App struct {\n\tData *preview.Data\n\tTheme string\n}\n\nfunc New(d *preview.Data) *App {\n\treturn &App{Data: d}\n}"),
			),
			dir("sailry-go/internal/kit",
				f("sailry-go/internal/kit/floating.go", "package kit\n\n// Frosted is the material of floating layers.\nfunc Frosted(c *ui.Context) ui.Material {\n\treturn frosted{}\n}"),
			),
		),
		f("sailry-go/README.md", "# Sailry\n\nA native desktop Agent workspace.\n\n## UI structure\n\n- Navigation, main content and context columns\n- Shared resource headers for conversations, files and development tools\n\nThis is a bundled example; no real project is read."),
		f("sailry-go/go.mod", "module github.com/sailry/sailry-go\n\ngo 1.27.3\n\nrequire github.com/egoist/mygo v0.8.3"),
	)
}

// hunk builds a hunk from lines marked "+", "-" or " ".
func hunk(old, new int, lines ...string) Hunk {
	h := Hunk{}
	o, n, add, del := old, new, 0, 0
	for _, l := range lines {
		d := DiffLine{Text: l[1:]}
		switch l[0] {
		case '+':
			d.Kind, d.New = Added, n
			n++
			add++
		case '-':
			d.Kind, d.Old = Removed, o
			o++
			del++
		default:
			d.Old, d.New = o, n
			o++
			n++
		}
		h.Lines = append(h.Lines, d)
	}
	h.Header = fmt.Sprintf("@@ -%d,%d +%d,%d @@", old, o-old, new, n-new)
	return h
}

// Texts returns the file before and after the change, for a diff
// viewer: the hunks' lines at their numbers, the lines between them,
// which the preview has not, blank.
func (f FileChange) Texts() (old, new string) {
	var o, n []string
	for _, h := range f.Hunks {
		for _, l := range h.Lines {
			for l.Kind != Added && len(o) < l.Old-1 {
				o = append(o, "")
			}
			for l.Kind != Removed && len(n) < l.New-1 {
				n = append(n, "")
			}
			if l.Kind != Added {
				o = append(o, l.Text)
			}
			if l.Kind != Removed {
				n = append(n, l.Text)
			}
		}
	}
	return strings.Join(o, "\n") + "\n", strings.Join(n, "\n") + "\n"
}

func change(path, status string, hunks ...Hunk) FileChange {
	c := FileChange{Path: path, Status: status, Hunks: hunks}
	for _, h := range hunks {
		for _, l := range h.Lines {
			switch l.Kind {
			case Added:
				c.Added++
			case Removed:
				c.Removed++
			}
		}
	}
	return c
}

func workspace() *Workspace {
	w := &Workspace{
		Hosts: []Host{
			{ID: "local", Name: "local_host_name", Local: true},
			{ID: "remote", Name: "remote_host_name", Update: true},
		},
		Files: files(),
	}
	// The reference's two diffs (diff_intro, diff_design) and two more.
	w.Changes = []FileChange{
		change("README.md", "M", hunk(1, 1,
			" # Sailry", " ", "-workspace: nested cards", "+workspace: three flat panels", " ",
			"-toolbar: global resource actions", "+toolbar: current resource header")),
		change("docs/example.md", "M", hunk(1, 1,
			" # Desktop layout", "-search: permanent search row", "+search: command palette", " ",
			"-theme: page-specific colors", "+theme: semantic theme tokens"),
			hunk(14, 14, " Narrow windows collapse the right column first;", "-manual choices are lost.", "+manual choices are kept when width returns.")),
		change("internal/kit/floating.go", "A", hunk(0, 1,
			"+package kit", "+", "+// Frosted is the material of floating layers.", "+func Frosted(c *ui.Context) ui.Material {",
			"+\treturn frosted{}", "+}")),
		change("internal/shell/legacy.go", "D", hunk(1, 0, "-package shell", "-", "-// legacy is gone.", "-var legacy = true")),
	}
	w.Checkpoints = []Checkpoint{
		{ID: "cp-3", Turn: "Make the borders lighter", Time: Now.Add(-4 * time.Minute), Files: []string{"internal/theme/controls.go", "internal/kit/loader.go"}, Added: 65, Removed: 3},
		{ID: "cp-2", Turn: "Port the neutral theme tokens", Time: Now.Add(-26 * time.Minute), Files: []string{"internal/theme/neutral.go"}, Added: 218, Removed: 0},
		{ID: "cp-1", Turn: "Copy locale keys for the shell", Time: Now.Add(-3 * time.Hour), Files: []string{"internal/i18n/keys.txt", "internal/i18n/locales/en.yml", "internal/i18n/locales/zh-CN.yml"}, Added: 42, Removed: 6},
	}
	w.Documents = []Document{
		{ID: "plan", Title: "Phase 3 plan", Path: "plans/phase-3.md", Text: "# Phase 3\n\n- Rail, header and frame motion\n- Sidebar groups, menus, rename and reorder\n- Right panel tabs with files, diffs and checkpoints\n\nPreview only; nothing is written."},
		{ID: "report", Title: "Benchmark report", Path: "reports/bench.md", Text: "# Benchmarks\n\nSteady 0.17 ms, Hover 0.18 ms, Scroll 0.56 ms at 200 iterations."},
	}
	w.Citations = []Citation{{
		Link:  "sailry-knowledge://docs/frame-motion?revision=3",
		Title: "Frame motion", Host: "local_host_name", Path: "docs/shell/frame.md", Start: 12, End: 30,
		Pages: []string{
			"The sidebar and the details panel open and close over 180 ms along cubic-bezier(0.2, 0, 0, 1).\nTheir content keeps its full width while the column is clipped, so it slides instead of reflowing.",
			"With Reduce Motion the panels jump to their new width.\nA narrow window hides the details panel without changing the saved choice.",
		},
	}}
	w.Notices = []*Notice{
		{ID: "n1", Kind: NoticeApproval, Title: "Tester coverage for the composer", Message: "Write file", Session: "sailry-go/tests", Host: "local_host_name", Time: Now},
		{ID: "n2", Kind: NoticeCompleted, Title: "Port the neutral theme tokens", Message: "Completed in 4m 12s", Session: "sailry-go/theme", Host: "local_host_name", Time: Now.Add(-5 * time.Minute)},
		{ID: "n3", Kind: NoticeInput, Title: "Flaky integration test", Message: "Which database should the test use?", Session: "api/tests", Host: "remote_host_name", Time: Now.Add(-40 * time.Minute)},
		{ID: "n4", Kind: NoticeFailed, Title: "Stabilize MCP warm-up tests", Message: "The provider closed the connection", Session: "sailry/mcp", Host: "local_host_name", Time: Now.Add(-2 * time.Hour), Read: true},
		{ID: "n5", Kind: NoticeInfo, Title: "Frame motion", Message: "docs/shell/frame.md", Link: "sailry-knowledge://docs/frame-motion?revision=3", Host: "local_host_name", Time: Now.Add(-3 * time.Hour)},
		{ID: "n6", Kind: NoticeCompleted, Title: "Mutable release assets", Message: "Completed in 1m 03s", Session: "sailry/assets", Host: "local_host_name", Time: Now.Add(-3 * 24 * time.Hour), Read: true},
	}
	return w
}

// Selection is the text the selection shortcut captures in the preview,
// which reads no other app.
const Selection = "Preview selection: the frame's columns open and close along one 180 ms curve, and a narrow window hides the right panel without forgetting it."
