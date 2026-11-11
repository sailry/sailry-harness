package live

import (
	"errors"
	"reflect"
	"slices"
	"testing"
	"testing/synctest"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/git"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// worktreeManagerEntries checks that the manager lists every worktree with
// its markers and offers only the actions each one can take.
func worktreeManagerEntries(t *testing.T) {
	branch := func(name string) *string { return &name }
	head := "0123456789abcdef0123456789abcdef01234567"
	trees := protocol.GitWorktrees{Entries: []protocol.GitWorktree{
		{Path: "/repo", Branch: branch("main"), Head: &head, Main: true, Available: true},
		{Path: "/trees/feature", Branch: branch("feature"), Head: &head, Available: true},
		{Path: "/trees/current", Branch: branch("current"), Head: &head, Available: true},
		{Path: "/trees/locked", Branch: branch("locked"), Head: &head, Locked: true, Available: true},
		{Path: `C:\trees\detached\`, Head: &head, Available: true},
		{Path: "/trees/gone", Branch: branch("gone"), Head: &head},
		{Path: "/trees/gone-locked", Branch: branch("kept"), Head: &head, Locked: true},
	}}
	entries := worktreeEntries(trees, "/trees/current")
	if len(entries) != len(trees.Entries) {
		t.Fatalf("manager lists %d of %d worktrees", len(entries), len(trees.Entries))
	}
	type row struct {
		label                        string
		marks                        []string
		current, stale, open, remove bool
	}
	want := []row{
		{label: "main", marks: []string{"worktree_main"}, open: true},
		{label: "feature", open: true, remove: true},
		{label: "current", current: true},
		{label: "locked", marks: []string{"worktree_locked"}, open: true},
		{label: "01234567", open: true},
		{label: "gone", marks: []string{"worktree_prune_on_select"}, stale: true},
		{label: "kept", marks: []string{"worktree_locked", "worktree_unavailable"}},
	}
	for index, entry := range entries {
		got := row{entry.label, entry.marks, entry.current, entry.stale, entry.open, entry.remove}
		if !reflect.DeepEqual(got, want[index]) {
			t.Errorf("%s: %+v, want %+v", entry.tree.Path, got, want[index])
		}
	}
	for path, name := range map[string]string{"/trees/feature": "feature", `C:\trees\detached\`: "detached", "/": "/", "plain": "plain"} {
		if got := worktreeName(path); got != name {
			t.Errorf("worktreeName(%q) = %q, want %q", path, got, name)
		}
	}
	if label := worktreeLabel(protocol.GitWorktree{Path: "/trees/unborn"}); label != "unborn" {
		t.Errorf("a worktree without a branch or commit is labelled %q", label)
	}
}

// workspaceFailures checks that Git and worktree failures name their cause
// where the Node distinguishes it, and that every name has copy.
func workspaceFailures(t *testing.T) {
	fault := func(code protocol.ErrorCode, message string) error {
		return &link.FaultError{Fault: protocol.Fault{Code: code, Message: message}}
	}
	cases := []struct {
		kind    string
		err     error
		outcome kit.Outcome
		key     string
	}{
		{worktreeCreate, fault(protocol.ErrorCodeConflict, git.BranchExists), kit.Conflict, "git_branch_exists"},
		{worktreeCreate, fault(protocol.ErrorCodeConflict, git.DestinationExists), kit.Conflict, "worktree_path_exists"},
		{worktreeCreate, fault(protocol.ErrorCodeConflict, "worktree directory is already registered"), kit.Conflict, "worktree_conflict"},
		{worktreeCreate, fault(protocol.ErrorCodeInvalidRequest, "expected an absolute new directory"), kit.Failed, "worktree_invalid"},
		{worktreeCreate, fault(protocol.ErrorCodeNotFound, ""), kit.Failed, "worktree_not_found"},
		{worktreeCreate, fault(protocol.ErrorCodeOutcomeUnknown, ""), kit.Uncertain, "worktree_outcome_unknown"},
		{worktreeOpen, fault(protocol.ErrorCodeConflict, "worktree is registered to another project"), kit.Conflict, "worktree_conflict"},
		{worktreeOpen, errors.New("offline"), kit.Failed, "worktree_request_failed"},
		{worktreeRemove, fault(protocol.ErrorCodeBusy, git.WorktreeLocked), kit.Failed, "worktree_remove_locked"},
		{worktreeRemove, fault(protocol.ErrorCodeBusy, "worktree has active commands"), kit.Failed, "worktree_remove_busy"},
		{worktreeRemove, fault(protocol.ErrorCodeConflict, git.WorktreeDirty), kit.Conflict, "worktree_remove_dirty"},
		{worktreeRemove, fault(protocol.ErrorCodeRevisionConflict, ""), kit.Conflict, "worktree_remove_conflict"},
		{worktreeRemove, fault(protocol.ErrorCodeNotFound, ""), kit.Failed, "worktree_remove_missing"},
		{worktreeRemove, fault(protocol.ErrorCodeOutcomeUnknown, ""), kit.Uncertain, "worktree_outcome_unknown"},
		{worktreePrune, fault(protocol.ErrorCodeBusy, git.WorktreeLocked), kit.Failed, "worktree_remove_locked"},
		{worktreePrune, fault(protocol.ErrorCodeBusy, "Git index is locked"), kit.Failed, "git_index_busy"},
		{worktreePrune, fault(protocol.ErrorCodeConflict, git.WorktreePathExists), kit.Conflict, "worktree_prune_exists"},
		{worktreePrune, fault(protocol.ErrorCodeRevisionConflict, ""), kit.Conflict, "git_index_conflict"},
		{worktreePrune, fault(protocol.ErrorCodeInvalidRequest, ""), kit.Failed, "worktree_invalid"},
		{worktreePrune, fault(protocol.ErrorCodeUnavailable, ""), kit.Failed, "worktree_prune_failed"},
		{gitIndex, fault(protocol.ErrorCodeRevisionConflict, ""), kit.Conflict, "git_index_changed"},
		{gitIndex, fault(protocol.ErrorCodeBusy, "Git index is locked"), kit.Failed, "git_index_busy"},
		{gitIndex, fault(protocol.ErrorCodePermissionDenied, ""), kit.Failed, "git_index_denied"},
		{gitIndex, fault(protocol.ErrorCodeNotFound, ""), kit.Failed, "git_index_missing"},
		{gitIndex, fault(protocol.ErrorCodeOutcomeUnknown, ""), kit.Uncertain, "git_index_unknown"},
		{gitIndex, fault(protocol.ErrorCodeInvalidRequest, ""), kit.Failed, "git_index_failed"},
		{gitCommit, fault(protocol.ErrorCodeNotConfigured, ""), kit.Failed, "git_identity_missing"},
		{gitCommit, fault(protocol.ErrorCodeUnavailable, ""), kit.Failed, "git_commit_unavailable"},
		{gitCommit, fault(protocol.ErrorCodeConflict, ""), kit.Conflict, "git_commit_failed"},
		{gitCommit, fault(protocol.ErrorCodeRevisionConflict, ""), kit.Conflict, "git_index_conflict"},
		{gitCommit, fault(protocol.ErrorCodeOutcomeUnknown, ""), kit.Uncertain, "git_commit_unknown"},
		{gitSwitch, fault(protocol.ErrorCodeConflict, git.BranchInWorktree), kit.Conflict, "git_branch_checked_out"},
		{gitSwitch, fault(protocol.ErrorCodeConflict, ""), kit.Conflict, "git_branch_conflict"},
		{gitSwitch, fault(protocol.ErrorCodeBusy, ""), kit.Failed, "git_branch_busy"},
		{gitSwitch, fault(protocol.ErrorCodeRevisionConflict, ""), kit.Conflict, "git_branch_revision"},
		{gitSwitch, fault(protocol.ErrorCodeInvalidRequest, ""), kit.Failed, "git_branch_failed"},
		{gitSwitch, fault(protocol.ErrorCodeOutcomeUnknown, ""), kit.Uncertain, "git_branch_unknown"},
		{gitDelete, fault(protocol.ErrorCodeConflict, git.BranchInWorktree), kit.Conflict, "git_branch_checked_out"},
		{gitDelete, fault(protocol.ErrorCodeConflict, ""), kit.Conflict, "git_branch_delete_conflict"},
		{gitDelete, fault(protocol.ErrorCodeInvalidRequest, ""), kit.Failed, "git_branch_delete_failed"},
		{gitDelete, fault(protocol.ErrorCodeOutcomeUnknown, ""), kit.Uncertain, "git_branch_delete_unknown"},
		{gitMerge, fault(protocol.ErrorCodeConflict, ""), kit.Conflict, "git_merge_conflict"},
		{gitMerge, fault(protocol.ErrorCodeNotConfigured, ""), kit.Failed, "git_identity_missing"},
		{gitMerge, fault(protocol.ErrorCodeInvalidRequest, ""), kit.Failed, "git_merge_failed"},
		{gitMerge, fault(protocol.ErrorCodeOutcomeUnknown, ""), kit.Uncertain, "git_merge_unknown"},
		{gitCreate, fault(protocol.ErrorCodeConflict, git.BranchExists), kit.Conflict, "git_branch_exists"},
		{gitCreate, fault(protocol.ErrorCodeInvalidRequest, ""), kit.Failed, "git_branch_create_failed"},
		{gitCreate, fault(protocol.ErrorCodeOutcomeUnknown, ""), kit.Uncertain, "git_branch_create_unknown"},
		{gitRename, fault(protocol.ErrorCodeConflict, git.BranchExists), kit.Conflict, "git_branch_exists"},
		{gitRename, fault(protocol.ErrorCodeConflict, ""), kit.Conflict, "git_branch_rename_conflict"},
		{gitRename, fault(protocol.ErrorCodeInvalidRequest, ""), kit.Failed, "git_branch_rename_failed"},
		{gitRename, fault(protocol.ErrorCodeOutcomeUnknown, ""), kit.Uncertain, "git_branch_rename_unknown"},
		{gitOperation, fault(protocol.ErrorCodePermissionDenied, ""), kit.Failed, "git_auth_failed"},
		{gitOperation, fault(protocol.ErrorCodeConflict, ""), kit.Conflict, "git_action_conflict"},
		{gitOperation, fault(protocol.ErrorCodeInvalidRequest, ""), kit.Failed, "git_action_failed"},
		{gitOperation, fault(protocol.ErrorCodeOutcomeUnknown, ""), kit.Uncertain, "git_action_unknown"},
		{fileSave, fault(protocol.ErrorCodePermissionDenied, ""), kit.Failed, "workspace_action_failed"},
		{fileSave, fault(protocol.ErrorCodeConflict, ""), kit.Conflict, "workspace_changed"},
		{fileSave, fault(protocol.ErrorCodeOutcomeUnknown, ""), kit.Uncertain, "workspace_action_unknown"},
	}
	// A save the file's change refused has no toast: the editor asks instead.
	for _, code := range []protocol.ErrorCode{protocol.ErrorCodeRevisionConflict, protocol.ErrorCodeNotFound} {
		if _, key := workspaceFailure(fileSave, fault(code, "")); key != "" {
			t.Errorf("refused save %v toasts %q", code, key)
		}
	}
	for _, lang := range []string{"en", "zh-CN"} {
		keys := i18n.Keys(lang)
		for _, test := range cases {
			outcome, key := workspaceFailure(test.kind, test.err)
			if outcome != test.outcome || key != test.key {
				t.Errorf("%s %v: %v %q, want %v %q", test.kind, test.err, outcome, key, test.outcome, test.key)
			}
			if !slices.Contains(keys, key) {
				t.Errorf("%s has no copy for %q", lang, key)
			}
		}
	}
	for _, key := range []string{"worktree_main", "worktree_locked", "worktree_unavailable", "worktree_prune_on_select", "worktree_listing_partial",
		"worktree_base", "worktree_path", "worktree_actions", "workspace_open_worktree", "worktree_load_failed", "worktree_missing_project",
		"worktree_no_repository", "worktree_revision_failed", "worktree_context_changed", "worktree_open_busy", "worktree_created",
		"worktree_removed", "worktree_remove_confirm", "worktree_prune", "worktree_prune_confirm", "worktree_pruned", "worktree_unused"} {
		if !slices.Contains(i18n.Keys("en"), key) || !slices.Contains(i18n.Keys("zh-CN"), key) {
			t.Errorf("the worktree manager has no copy for %q", key)
		}
	}
}

// worktreeManagerUnused checks which worktrees the manager marks as used by
// no session: the project's linked ones only.
func worktreeManagerUnused(t *testing.T) {
	project, other := protocol.ProjectID{1}, protocol.ProjectID{2}
	tree := func(id byte, owner *protocol.ProjectID, path string, main bool) protocol.Worktree {
		return protocol.Worktree{ID: protocol.WorktreeID{id}, Project: owner, Path: path, Main: main}
	}
	snapshot := protocol.Snapshot{
		Worktrees: []protocol.Worktree{tree(1, &project, "/repo", true), tree(2, &project, "/repo/.sailry/worktrees/used", false),
			tree(3, &project, "/repo/.sailry/worktrees/left", false), tree(4, &other, "/other/.sailry/worktrees/left", false), tree(5, nil, "/profile/workspaces/sessions/one", false)},
		Sessions: []protocol.Session{{Worktree: protocol.WorktreeID{2}}},
	}
	if got := unusedWorktrees(snapshot, &project); len(got) != 1 || !got["/repo/.sailry/worktrees/left"] {
		t.Fatalf("unused worktrees = %v", got)
	}
	if got := unusedWorktrees(snapshot, nil); len(got) != 0 {
		t.Fatalf("a directory without a project marks %v", got)
	}
}

// worktreeManagerLive drives the manager's rows against a Node that answers
// captured responses: the list's copy, the actions each row offers, opening a
// worktree for an unsent draft and the toast of a named failure.
func worktreeManagerLive(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		root, source := terminalTabsFixture(t)
		root.initWorkspace()
		project := protocol.ProjectID{1}
		tree := protocol.Worktree{ID: protocol.WorktreeID{1}, Project: &project, Path: "/fixture", Main: true}
		const draft = "draft/worktrees"
		node := protocol.NodeID{1}.String()
		root.App.Data.Projects = []*viewmodel.Project{{ID: project.String(), Host: node, Conversations: []*viewmodel.Conversation{{ID: draft, Node: node}}}}
		root.App.Sidebar.Selected = draft
		comp := root.App.Composer(draft)
		comp.Project = project.String()
		p := root.workspaceFor(tree)
		root.workspace.selected = tree.ID

		head, main, feature, gone := "0123456789abcdef0123456789abcdef01234567", "main", "feature", "gone"
		listing := protocol.GitWorktrees{Kind: protocol.RepositoryKindReady, Entries: []protocol.GitWorktree{
			{Path: "/fixture", Branch: &main, Head: &head, Main: true, Available: true},
			{Path: "/trees/feature", Branch: &feature, Head: &head, Available: true},
			{Path: "/trees/gone", Branch: &gone, Head: &head},
		}}
		registered := protocol.Worktree{ID: protocol.WorktreeID{2}, Project: &project, Path: "/trees/feature"}
		refuse := true
		source.respond = func(request protocol.Request) link.Response {
			switch request.Command.Kind {
			case "register_worktree":
				if refuse {
					return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "worktree is registered to another project"}}
				}
				return link.Response{Output: protocol.Output{Kind: "worktree", Data: registered}}
			}
			return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "fixture command is unavailable"}}
		}

		var (
			toasts []string
			act    func(*ui.Context)
		)
		tester := ui.NewTester(func(c *ui.Context) {
			if act != nil {
				act(c)
				act = nil
			}
			root.mu.Lock()
			queue := root.queue
			root.queue = nil
			root.mu.Unlock()
			for _, post := range queue {
				post(c)
			}
			ui.ToastViewportBase(c, func(_ ui.Element, shown []ui.Toast) {
				toasts = toasts[:0]
				for _, toast := range shown {
					toasts = append(toasts, toast.Title)
				}
			})
		}, 640, 400)
		tester.SetLocale("en")
		frame := func(action func(*ui.Context)) {
			act = action
			tester.Frame()
			synctest.Wait()
			tester.Frame()
		}

		var rows, current, linked []panel.PickerRow
		frame(func(c *ui.Context) {
			rows = p.worktreeRows(c, listing)
			entries := worktreeEntries(listing, p.owner.Path)
			current, linked = p.worktreeActions(c, entries[0]), p.worktreeActions(c, entries[1])
		})
		type shown struct {
			label, detail, group string
			checked, chosen      bool
		}
		want := []shown{
			{"main", "Main · fixture", "Worktrees", true, true},
			{"feature", "feature", "Worktrees", false, true},
			{"gone", "Unavailable · click to clean up · gone", "Worktrees", false, true},
			{"New worktree", "", "Worktree actions", false, true},
		}
		if len(rows) != len(want) {
			t.Fatalf("manager shows %d rows, want %d", len(rows), len(want))
		}
		for index, row := range rows {
			if got := (shown{row.Label, row.Detail, row.Group, row.Checked, row.Run != nil && !row.Disabled}); got != want[index] {
				t.Errorf("row %d: %+v, want %+v", index, got, want[index])
			}
		}
		labels := func(rows []panel.PickerRow) []string {
			var labels []string
			for _, row := range rows {
				labels = append(labels, row.Label)
			}
			return labels
		}
		if got := labels(current); !slices.Equal(got, []string{"Copy path"}) {
			t.Errorf("the current main worktree offers %v", got)
		}
		if got := labels(linked); !slices.Equal(got, []string{"Open worktree", "Copy path", "Remove worktree"}) {
			t.Fatalf("a linked worktree offers %v", got)
		}

		frame(linked[0].Run)
		if comp.Worktree != tree.Path || !slices.Contains(toasts, i18n.In("en", "worktree_conflict")) {
			t.Fatalf("a refused registration left the draft at %q with toasts %v", comp.Worktree, toasts)
		}
		refuse = false
		frame(linked[0].Run)
		if comp.Worktree != registered.Path || root.workspace.admitted[registered.ID].Path != registered.Path {
			t.Fatalf("Open worktree left the draft at %q", comp.Worktree)
		}
		if sent := source.sent("register_worktree"); len(sent) != 2 {
			t.Fatalf("register_worktree commands = %d, want 2", len(sent))
		}

		frame(func(*ui.Context) { p.manageWorktrees() })
		if !slices.Contains(toasts, i18n.In("en", "worktree_load_failed")) {
			t.Fatalf("a failed listing showed %v", toasts)
		}
	})
}
