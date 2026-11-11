package live

import (
	"fmt"
	"reflect"
	"testing"

	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestPanelProjection(t *testing.T) {
	t.Run("panel tabs close, stay closed and never show a blank body", panelTabs)
	t.Run("browser tab shows its page's state and fills its body", browserChrome)
	t.Run("rail Files and Git pages show the selected project", railPages)
	t.Run("worktree manager lists every worktree with its markers and actions", worktreeManagerEntries)
	t.Run("Git and worktree failures name their cause", workspaceFailures)
	t.Run("worktree manager rows act against the Node", worktreeManagerLive)
	t.Run("manual hunk selection retains displayed coordinate identity", func(t *testing.T) {
		patch := "@@ -1,2 +1,2 @@\n-before\n+after\n tail\n@@ -20,2 +20,2 @@\n-old\n+new\n end\n"
		hunks := workspaceHunks(patch)
		if len(hunks) != 2 || workspaceManualHunkIndex(hunks[1], patch) != 1 {
			t.Fatal("manual selection lost captured patch position")
		}
		stale := hunks[1]
		stale.Lines = append([]viewmodel.DiffLine(nil), stale.Lines...)
		stale.Lines[1].Text = "unrelated"
		if workspaceManualHunkIndex(stale, patch) != -1 {
			t.Fatal("manual hunk accepted unrelated changed lines")
		}
	})
	t.Run("captured Git document projection retains flags and exact patch", func(t *testing.T) {
		projected := workspaceGitDocumentFile(protocol.GitDiff{Path: "source.go", Text: "@@ -1 +1 @@\n-before Ω\n+after λ\n", Additions: 1, Deletions: 1, Binary: false, Truncated: true})
		if projected.Path != "source.go" || projected.Added != 1 || projected.Removed != 1 || projected.Binary || !projected.Truncated || len(projected.Hunks) != 1 || projected.Hunks[0].Lines[1].Text != "after λ" || projected.Hunks[0].Lines[1].New != 1 {
			t.Fatalf("document projection=%#v", projected)
		}
	})
	t.Run("tracking switch uses existing mapping and longest remote prefix", func(t *testing.T) {
		branches := protocol.GitBranches{Sync: protocol.GitSync{Remotes: []string{"origin", "origin/team"}, Tracking: map[string]string{"local": "origin/topic"}}, Entries: []protocol.GitBranch{{Name: "local"}, {Name: "topic"}}}
		if local, collision := workspaceTrackingBranch(branches, "origin/topic"); local != "local" || collision {
			t.Fatalf("tracked local = %q conflict=%v", local, collision)
		}
		if local, collision := workspaceTrackingBranch(branches, "origin/team/topic"); local != "topic" || !collision {
			t.Fatalf("longest remote collision = %q conflict=%v", local, collision)
		}
		if local, collision := workspaceTrackingBranch(branches, "unknown/topic"); local != "" || collision {
			t.Fatal("unrecognized remote was converted to local branch")
		}
	})
	t.Run("history projects actual page offset without mutating canonical head", func(t *testing.T) {
		data := &viewmodel.Workspace{}
		p := &workspaceProject{data: data, gitLogPages: []protocol.GitLog{{Entries: []protocol.GitLogEntry{{ID: "head", Message: "latest"}}}, {Offset: 100, ReferencesTruncated: true, Entries: []protocol.GitLogEntry{{ID: "older", Message: "older"}}}}, gitLogPage: 1}
		p.panel = panel.NewLiveProject("worktree", data, &panel.Actions{})
		p.projectGitHistory(false)
		if len(data.Repo.Commits) != 1 || data.Repo.Commits[0].ID != "older" {
			t.Fatalf("history projection=%#v", data.Repo.Commits)
		}
		p.gitHistoryPage(false)
		if data.Repo.Commits[0].ID != "head" {
			t.Fatal("previous page did not restore cached canonical page")
		}
	})
	t.Run("directory refresh preserves visible references and deduplicates pages", func(t *testing.T) {
		kept := &viewmodel.FileNode{Name: "a.go", Path: "a.go", Text: "loaded"}
		data := &viewmodel.Workspace{Files: &viewmodel.FileNode{Children: []*viewmodel.FileNode{kept, {Name: "removed", Path: "removed"}}}}
		p := &workspaceProject{data: data, directories: map[string]bool{}}
		p.panel = panel.NewLiveProject("worktree", data, &panel.Actions{})
		entries := []protocol.FileEntry{{Name: "a.go", Kind: protocol.EntryKindFile}, {Name: "folder", Kind: protocol.EntryKindDirectory}, {Name: "a.go", Kind: protocol.EntryKindFile}}
		p.directoryPage(nil, "", protocol.Directory{}, entries)
		if len(data.Files.Children) != 2 || data.Files.Children[0] != kept || kept.Text != "loaded" || data.Files.Children[1].Children == nil || len(p.directoryEntries[""]) != 2 {
			t.Fatalf("directory projection lost references or duplicated names: %#v", data.Files.Children)
		}
		p.panel.SetFile("a.go", "loaded\n")
		p.fileUnavailable("a.go")
		if kept.Text != "" {
			t.Fatal("failed clean read retained stale text")
		}
		p.panel.SetFile("a.go", "loaded\n")
		p.panel.FileChanged("a.go", "unsaved")
		p.fileUnavailable("a.go")
		if draft, dirty := p.panel.FileDraft("a.go"); draft != "unsaved\n" || !dirty {
			t.Fatal("failed read erased the dirty draft")
		}
	})
	t.Run("search captures content options and bounded globs", func(t *testing.T) {
		options := workspaceSearchOptions(panel.SearchQuery{Query: "needle", Filter: "  *.go  \n\n !vendor/** \n", CaseSensitive: true, Regex: true})
		if options.Query != "needle" || options.Mode != protocol.SearchModeContent || options.Limit != 200 || !options.CaseSensitive || !options.Regex || !reflect.DeepEqual(options.Globs, []string{"*.go", "!vendor/**"}) {
			t.Fatalf("search options = %#v", options)
		}
	})
	t.Run("status and file references", func(t *testing.T) {
		data := &viewmodel.Workspace{Files: &viewmodel.FileNode{Name: "workspace", Children: []*viewmodel.FileNode{}}}
		p := &workspaceProject{data: data}
		p.panel = panel.NewLiveProject("worktree", data, &panel.Actions{})
		p.ensureFile("nested/readme.md")
		p.panel.SetFile("nested/readme.md", "original\n")
		node := workspaceFileNode(data.Files, "nested/readme.md")
		if node == nil || node.Name != "readme.md" || node.Text != "original\n" {
			t.Fatalf("nested file projection = %#v", node)
		}
		p.ensureFile("nested/readme.md")
		if len(data.Files.Children) != 1 || len(data.Files.Children[0].Children) != 1 {
			t.Fatal("opening a reference duplicated the tree")
		}
		staged, unstaged := protocol.GitChangeKindAdded, protocol.GitChangeKindDeleted
		branch := "feature"
		p.projectStatus(protocol.GitStatus{Branch: &branch, Entries: []protocol.GitEntry{{Path: "added.go", Staged: &staged}, {Path: "deleted.go", Unstaged: &unstaged}, {Path: "new.go", Untracked: true}}})
		if data.Repo.Branch != "feature" || len(data.Changes) != 3 || data.Changes[0].Staged != "A" || data.Changes[1].Status != "D" || data.Changes[2].Status != "A" {
			t.Fatalf("status projection = %#v", data)
		}
		if p.panel.Git.Change() != "added.go" {
			t.Fatal("first real change did not become selected")
		}
		p.projectStatus(protocol.GitStatus{})
		if len(data.Changes) != 0 {
			t.Fatal("removed changes remained visible")
		}
	})
	t.Run("hunk identity survives reordered reviews", func(t *testing.T) {
		first := "@@ -1,3 +1,3 @@\n alpha\n-old\n+new\n omega\n"
		other := "@@ -7,3 +7,3 @@\n middle\n-before\n+after\n end\n"
		displayed := workspaceHunks(first)
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
	t.Run("commit scopes keep staged intent", func(t *testing.T) {
		kind := protocol.GitChangeKindModified
		status := protocol.GitStatus{Entries: []protocol.GitEntry{{Path: "a", Staged: &kind}, {Path: "b", Unstaged: &kind}}}
		if tracked, all := workspaceCommitScope(status, "auto"); tracked || all {
			t.Fatal("automatic commit included unstaged changes despite staged content")
		}
		if tracked, all := workspaceCommitScope(status, "all"); tracked || !all {
			t.Fatal("all commit excluded untracked content")
		}
		status.Entries[0].Staged = nil
		if tracked, all := workspaceCommitScope(status, "auto"); !tracked || all {
			t.Fatal("automatic commit did not use tracked changes")
		}
	})
	t.Run("native changes use timestamp order and complete totals", func(t *testing.T) {
		status := protocol.GitStatus{Kind: protocol.RepositoryKindReady, Truncated: true}
		for index := range 12 {
			modified := uint64(index)
			status.Entries = append(status.Entries, protocol.GitEntry{Path: fmt.Sprintf("file-%02d", index), ModifiedMs: &modified, Diff: protocol.GitLineStats{Additions: uint64(index), Deletions: 1}})
		}
		projected := workspaceChanges(status)
		if projected.Files != 12 || projected.Added != 66 || projected.Removed != 12 || !projected.Partial || len(projected.Recent) != 10 {
			t.Fatalf("native badge = %#v", projected)
		}
		if projected.Recent[0].Path != "file-11" || projected.Recent[9].Path != "file-02" {
			t.Fatalf("recent order = %#v", projected.Recent)
		}
		status.Kind = protocol.RepositoryKindDirectory
		if workspaceChanges(status) != nil {
			t.Fatal("plain directory received a Git badge")
		}
	})
	t.Run("turn changes retain captured paths and counts", func(t *testing.T) {
		projected := workspaceTurnFiles(protocol.ConversationCheckpointTurnDiff{Files: []protocol.GitDiff{{Path: "captured.go", Text: "@@ -1,2 +1,3 @@\n same\n-old\n+new\n+extra\n"}}})
		if len(projected) != 1 || projected[0].Path != "captured.go" || projected[0].Added != 2 || projected[0].Removed != 1 {
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
