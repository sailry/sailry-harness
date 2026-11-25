package live

import (
	"reflect"
	"testing"

	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestPanelProjection(t *testing.T) {
	t.Run("panel tabs close, stay closed and never show a blank body", panelTabs)
	t.Run("browser tab shows its page's state and fills its body", browserChrome)
	t.Run("rail Files page of a session without a project reads once", railPagesSession)
	t.Run("workspace failures name their cause", workspaceFailures)
	t.Run("failed reads retain a dirty editor", func(t *testing.T) {
		p := &workspaceProject{files: map[string]protocol.FileContent{}}
		p.panel = panel.NewLiveProject("worktree", &viewmodel.Workspace{}, &panel.Actions{})
		p.panel.SetFile("a.go", "loaded\n")
		p.panel.FileChanged("a.go", "unsaved")
		p.fileUnavailable("a.go")
		if draft, dirty := p.panel.FileDraft("a.go"); draft != "unsaved\n" || !dirty {
			t.Fatal("failed read erased the dirty draft")
		}
	})
	t.Run("draft keeps its source while the file changes on disk", func(t *testing.T) {
		data := &viewmodel.Workspace{Files: &viewmodel.FileNode{Children: []*viewmodel.FileNode{{Name: "a.go", Path: "a.go"}}}}
		p := &workspaceProject{data: data, files: map[string]protocol.FileContent{}}
		p.panel = panel.NewLiveProject("worktree", data, &panel.Actions{})
		first, second := "first", "second"
		p.fileLoaded("a.go", protocol.FileContent{Path: "a.go", Text: "one\n", Revision: &first})
		p.panel.FileChanged("a.go", "mine")
		p.fileLoaded("a.go", protocol.FileContent{Path: "a.go", Text: "theirs\n", Revision: &second})
		if draft, dirty := p.panel.FileDraft("a.go"); draft != "mine\n" || !dirty || *p.files["a.go"].Revision != first || p.changedFiles["a.go"].content.Text != "theirs\n" {
			t.Fatalf("a read under a draft replaced the draft or its source: %q %v %#v %#v", draft, dirty, p.files["a.go"], p.changedFiles["a.go"])
		}
		p.draftSource("a.go", changedFile{content: protocol.FileContent{Path: "a.go", Text: "one\n", Revision: &first}})
		if _, changed := p.changedFiles["a.go"]; changed {
			t.Fatal("a file back at the draft's source stayed marked as changed")
		}
		// Edits undone by hand leave nothing to keep: the shown file follows the disk.
		p.draftSource("a.go", changedFile{content: protocol.FileContent{Path: "a.go", Text: "theirs\n", Revision: &second}})
		p.panel.FileChanged("a.go", "one")
		p.readFile("a.go")
		if draft, dirty := p.panel.FileDraft("a.go"); draft != "theirs\n" || dirty || len(p.changedFiles) != 0 || *p.files["a.go"].Revision != second {
			t.Fatalf("an undone draft kept stale content: %q %v", draft, dirty)
		}
		p.panel.FileChanged("a.go", "mine")
		p.draftSource("a.go", changedFile{missing: true})
		p.fileUnavailable("a.go")
		if _, loaded := p.files["a.go"]; !loaded || !p.changedFiles["a.go"].missing {
			t.Fatal("a file gone under a draft lost the draft's source or its mark")
		}
		p.panel.DropFileDraft("a.go")
		p.fileLoaded("a.go", protocol.FileContent{Path: "a.go", Text: "theirs\n", Revision: &second})
		if draft, dirty := p.panel.FileDraft("a.go"); draft != "theirs\n" || dirty || len(p.changedFiles) != 0 || *p.files["a.go"].Revision != second {
			t.Fatalf("reload kept the draft or the old source: %q %v", draft, dirty)
		}
		p.fileUnavailable("a.go")
		if _, loaded := p.files["a.go"]; loaded {
			t.Fatal("a failed clean read kept its source")
		}
	})
	t.Run("search captures content options and bounded globs", func(t *testing.T) {
		options := workspaceSearchOptions(panel.SearchQuery{Query: "needle", Filter: "  *.go  \n\n !vendor/** \n", CaseSensitive: true, Regex: true})
		if options.Query != "needle" || options.Mode != protocol.SearchModeContent || options.Limit != 200 || !options.CaseSensitive || !options.Regex || !reflect.DeepEqual(options.Globs, []string{"*.go", "!vendor/**"}) {
			t.Fatalf("search options = %#v", options)
		}
	})
	t.Run("file references", func(t *testing.T) {
		data := &viewmodel.Workspace{}
		p := &workspaceProject{data: data, files: map[string]protocol.FileContent{}}
		p.panel = panel.NewLiveProject("worktree", data, &panel.Actions{})
		const name = "nested/readme.md"
		p.fileLoaded(name, protocol.FileContent{Path: name, Text: "original\n"})
		p.panel.FileOpen(name)
		p.panel.FileOpen(name)
		if text, dirty := p.panel.FileDraft(name); text != "original\n" || dirty || len(p.panel.Files.Open()) != 1 || data.Files != nil {
			t.Fatalf("opening a reference did not reuse its explicit editor: %q, dirty=%t, tabs=%v", text, dirty, p.panel.Files.Open())
		}
	})
	t.Run("hunk identity survives reordered reviews", func(t *testing.T) {
		first := "@@ -1,3 +1,3 @@\n alpha\n-old\n+new\n omega\n"
		other := "@@ -7,3 +7,3 @@\n middle\n-before\n+after\n end\n"
		displayed := viewmodel.ParseDiff(first)
		if len(displayed) != 1 || len(displayed[0].Lines) != 4 {
			t.Fatalf("parsed hunks = %#v", displayed)
		}
		reviews := []protocol.ConversationCheckpointHunk{{ID: "other", Text: other}, {ID: "wanted", Text: first}}
		if got := workspaceHunkMatch(displayed[0], reviews); got != 1 {
			t.Fatalf("matched index = %d", got)
		}
		reviews = append(reviews, reviews[1])
		if got := workspaceHunkMatch(displayed[0], reviews); got != -1 {
			t.Fatalf("ambiguous hunk matched %d", got)
		}
		reviews = []protocol.ConversationCheckpointHunk{{Text: other}}
		if got := workspaceHunkMatch(displayed[0], reviews); got != -1 {
			t.Fatalf("unrelated hunk matched %d", got)
		}
	})
	t.Run("turn changes retain captured paths and counts", func(t *testing.T) {
		diff := protocol.ConversationCheckpointTurnDiff{Files: []protocol.GitDiff{{Path: "captured.go", Additions: 5, Deletions: 1, Text: "@@ -1,2 +1,3 @@\n same\n-old\n+new\n+extra\n"}}}
		projected := workspaceTurnFiles(diff)
		summary := workspaceTurnSummary(diff)
		if len(summary) != 1 || summary[0].Added != 5 || summary[0].Removed != 1 || summary[0].Path != "captured.go" {
			t.Fatalf("turn summary = %#v", summary)
		}
		if len(projected) != 1 || projected[0].Path != "captured.go" || projected[0].Added != 5 || projected[0].Removed != 1 || len(projected[0].Hunks) != 1 {
			t.Fatalf("turn changes = %#v", projected)
		}
	})

	t.Run("entry destination edits preserve captured source owners", func(t *testing.T) {
		source, target := protocol.NewWorktreeID(), protocol.NewWorktreeID()
		original := protocol.Command{Kind: "move_entry_to", Data: protocol.CommandMoveEntryToPayload{Source: source, Worktree: target, From: "captured/file.txt", To: "target/file.txt"}}
		changed := workspaceTransferDestination(original, "target/renamed.txt")
		value, ok := detail[protocol.CommandMoveEntryToPayload](changed.Data)
		if !ok || value.Source != source || value.Worktree != target || value.From != "captured/file.txt" || value.To != "target/renamed.txt" {
			t.Fatalf("edited transfer lost provenance: %#v", changed)
		}
		prior, _ := detail[protocol.CommandMoveEntryToPayload](original.Data)
		if prior.To != "target/file.txt" {
			t.Fatal("destination draft mutated original queued command")
		}
	})

}
