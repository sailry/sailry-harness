package preview

import (
	"fmt"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"strings"
	"time"
)

type Host = viewmodel.Host
type Worktree = viewmodel.Worktree
type NoticeKind = viewmodel.NoticeKind
type Notice = viewmodel.Notice
type FileNode = viewmodel.FileNode
type LineKind = viewmodel.LineKind
type DiffLine = viewmodel.DiffLine
type Hunk = viewmodel.Hunk
type FileChange = viewmodel.FileChange
type Branch = viewmodel.Branch
type Remote = viewmodel.Remote
type Commit = viewmodel.Commit
type Repository = viewmodel.Repository
type Checkpoint = viewmodel.Checkpoint
type Document = viewmodel.Document
type Citation = viewmodel.Citation
type Workspace = viewmodel.Workspace

const (
	NoticeCompleted = viewmodel.NoticeCompleted
	NoticeFailed    = viewmodel.NoticeFailed
	NoticeApproval  = viewmodel.NoticeApproval
	NoticeInput     = viewmodel.NoticeInput
	NoticeInfo      = viewmodel.NoticeInfo
	Context         = viewmodel.Context
	Added           = viewmodel.Added
	Removed         = viewmodel.Removed
)

// files of the panel's tree: the reference's two and a few more, so the tree
// has depth.
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
	// Where each change stands: the README in the work tree, the example
	// staged and changed again, the new file untracked, the removal staged.
	w.Changes[0].Unstaged = "M"
	w.Changes[1].Staged, w.Changes[1].Unstaged = "M", "M"
	w.Changes[2].Untracked = true
	w.Changes[3].Staged = "D"
	w.Repo = Repository{
		Branch: "main", Upstream: "origin/main",
		Branches: []Branch{
			{Name: "main", Current: true, Commit: "9f2c41ab07d3", Tracking: "↑2"},
			{Name: "split-view", Commit: "4be81d20c9a7"},
			{Name: "feature/git-panel", Commit: "c07d5e3318f2", Tracking: "↓2"},
			{Name: "origin/main", Remote: true, Commit: "1a6e90f4b2c8"},
			{Name: "origin/split-view", Remote: true, Commit: "4be81d20c9a7"},
		},
		Remotes: []Remote{{Name: "origin", URL: "https://github.com/sailry/sailry-go.git"}},
		Stashes: []string{"WIP on main: 9f2c41a Port the Git panel"},
		Tags:    []string{"v0.1.0"},
		// Newest first, every child before its parents: split-view was merged
		// into main, which is two commits ahead of origin/main.
		Commits: []Commit{
			{ID: "9f2c41ab07d3e5f6", Message: "Port the Git panel's grouped changes", Author: "Ada Chen", Refs: []string{"HEAD", "heads/main"}, Parents: []string{"e5a31c7f9d0b4a68"}},
			{ID: "e5a31c7f9d0b4a68", Message: "Merge branch 'split-view'", Author: "Ada Chen", Refs: []string{}, Parents: []string{"1a6e90f4b2c87d10", "4be81d20c9a7e3b2"}},
			{ID: "1a6e90f4b2c87d10", Message: "Share one theme across the right panel\n\nThe tabs and pages read the same tokens.", Author: "Ada Chen", Refs: []string{"remotes/origin/main"}, Parents: []string{"c07d5e3318f2a9d4"}},
			{ID: "4be81d20c9a7e3b2", Message: "Split the workspace into panes", Author: "Lin Wei", Refs: []string{"heads/split-view", "remotes/origin/split-view"}, Parents: []string{"c07d5e3318f2a9d4"}},
			{ID: "c07d5e3318f2a9d4", Message: "Draw the file tree with guides", Author: "Lin Wei", Refs: []string{"heads/feature/git-panel"}, Parents: []string{"77b0f2e6a1d94c35"}},
			{ID: "77b0f2e6a1d94c35", Message: "Copy the shell's locale keys", Author: "Ada Chen", Refs: []string{"tags/v0.1.0"}, Parents: []string{}},
		},
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
