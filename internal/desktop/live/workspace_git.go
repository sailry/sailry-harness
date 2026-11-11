package live

import (
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func workspaceIndex(status protocol.GitStatus) string {
	if status.IndexRevision != nil {
		return *status.IndexRevision
	}
	return ""
}
func workspaceString(value *string) string {
	if value != nil {
		return *value
	}
	return ""
}
func (p *workspaceProject) gitAction(c *ui.Context, a panel.GitAction) {
	status := p.status
	runAs := func(kind string, action protocol.GitAction, finish func(bool)) {
		command := protocol.Command{Kind: "run_git_action", Data: protocol.CommandRunGitActionPayload{Worktree: p.owner.ID, Action: action, ExpectedIndex: workspaceIndex(status), ExpectedHead: status.Head, ExpectedBranch: status.Branch}}
		p.mutateNamed(kind, command, func(c *ui.Context, out protocol.Output) {
			if finish != nil {
				finish(out.Kind != "")
			}
			if out.Kind != "" {
				p.diffs = map[string]bool{}
				kit.Toast(c, kit.Done, i18n.T(c, "workspace_completed"))
			}
		})
	}
	run := func(action protocol.GitAction, finish func(bool)) { runAs(gitOperation, action, finish) }
	switch a.Kind {
	case "git_stage", "git_unstage", "git_stage_all", "git_unstage_all":
		operation := protocol.GitIndexChangeStage
		if strings.Contains(a.Kind, "unstage") {
			operation = protocol.GitIndexChangeUnstage
		}
		paths := a.Paths
		conflicts := map[string]bool{}
		for _, entry := range status.Entries {
			if entry.Conflicted {
				conflicts[entry.Path] = true
			}
		}
		if strings.HasSuffix(a.Kind, "_all") {
			for _, entry := range status.Entries {
				if !entry.Conflicted && (operation == protocol.GitIndexChangeStage && (entry.Unstaged != nil || entry.Untracked) || operation == protocol.GitIndexChangeUnstage && entry.Staged != nil) {
					paths = append(paths, entry.Path)
				}
			}
		}
		// Staging a conflicted file records its resolution; the panel sends
		// one only for that file's own row.
		eligible := paths[:0]
		for _, relative := range paths {
			if a.Kind == "git_stage" || !conflicts[relative] {
				eligible = append(eligible, relative)
			}
		}
		paths = eligible
		if len(paths) == 0 {
			return
		}
		p.mutateNamed(gitIndex, protocol.Command{Kind: "update_git_index", Data: protocol.CommandUpdateGitIndexPayload{Worktree: p.owner.ID, Paths: paths, Operation: operation, ExpectedIndex: workspaceIndex(status), ExpectedHead: status.Head}}, nil)
	case "git_commit":
		for _, entry := range status.Entries {
			if entry.Conflicted {
				p.workspaceError(c, protocol.ErrorCodeConflict, "resolve index conflicts before committing")
				return
			}
		}
		if a.Scope == "auto" && !a.Amend {
			staged, tracked := false, false
			for _, entry := range status.Entries {
				staged = staged || entry.Staged != nil
				tracked = tracked || !entry.Untracked
			}
			if !staged {
				choices := []panel.Choice{}
				for _, scope := range []string{"tracked", "all"} {
					if scope == "tracked" && !tracked {
						continue
					}
					choice := a
					choice.Scope = scope
					choices = append(choices, panel.Choice{Label: i18n.T(c, "git_commit_"+scope), Run: func(c *ui.Context) { p.gitAction(c, choice) }})
				}
				p.panel.Choose(i18n.T(c, "git_commit_scope"), choices)
				return
			}
		}
		after := protocol.GitAfterCommit(a.After)
		if after == "" {
			after = protocol.GitAfterCommitNone
		}
		if after != protocol.GitAfterCommitNone && p.branches.Sync.Upstream == nil && a.Remote == "" {
			choices := []panel.Choice{}
			for _, remote := range p.branches.Sync.Remotes {
				choice := a
				choice.Remote = remote
				choices = append(choices, panel.Choice{Label: remote, Run: func(c *ui.Context) { p.gitAction(c, choice) }})
			}
			if len(choices) == 0 {
				p.gitAction(c, panel.GitAction{Kind: "git_add_remote"})
			} else {
				p.panel.Choose(i18n.T(c, "git_publish"), choices)
			}
			return
		}
		var pushRemote *string
		if a.Remote != "" {
			pushRemote = &a.Remote
		}
		tracked, all := workspaceCommitScope(status, a.Scope)
		command := protocol.Command{Kind: "create_git_commit", Data: protocol.CommandCreateGitCommitPayload{Worktree: p.owner.ID, Message: a.Message, Amend: a.Amend, ExpectedIndex: workspaceIndex(status), ExpectedHead: status.Head, ExpectedBranch: status.Branch, Options: protocol.GitCommitOptions{Signoff: a.Signoff, SkipHooks: a.SkipHooks, Tracked: tracked, All: all, After: after, PushRemote: pushRemote}}}
		p.mutateNamed(gitCommit, command, func(c *ui.Context, out protocol.Output) {
			if out.Kind == "" {
				return
			}
			p.panel.CommitSaved(a.Message)
			if result, ok := detail[protocol.OutputGitCommitCreatedPayload](out.Data); ok && result.FollowUp != nil {
				p.runtime.reportKeys(c, result.FollowUp, "workspace_action_failed", "workspace_changed", "workspace_action_unknown")
				return
			}
			kit.Toast(c, kit.Done, i18n.T(c, "workspace_completed"))
		})
	case "git_discard_changes":
		run(protocol.GitAction{Kind: "discard", Data: protocol.GitActionDiscardPayload{Paths: a.Paths}}, nil)
	case "git_discard_all":
		run(protocol.GitAction{Kind: "discard_all", Data: protocol.GitActionDiscardAllPayload{Paths: a.Paths}}, nil)
	case "git_ignore":
		run(protocol.GitAction{Kind: "ignore", Data: protocol.GitActionIgnorePayload{Paths: a.Paths, Directories: a.Scope == "directory"}}, nil)
	case "git_undo_commit":
		run(protocol.GitAction{Kind: "undo_commit"}, nil)
	case "git_abort":
		run(protocol.GitAction{Kind: "abort"}, nil)
	case "git_sync", "git_sync_rebase":
		run(protocol.GitAction{Kind: "sync", Data: protocol.GitActionSyncPayload{Rebase: a.Kind == "git_sync_rebase"}}, nil)
	case "git_pull", "git_pull_rebase", "git_pull_from":
		input := protocol.GitActionPullPayload{Rebase: a.Kind == "git_pull_rebase"}
		if a.Target != "" {
			remote, branch, ok := workspaceRemoteBranch(p.branches.Sync.Remotes, a.Target)
			if !ok {
				p.workspaceError(c, protocol.ErrorCodeNotConfigured, "remote branch has no configured remote")
				return
			}
			input.Remote, input.Branch = &remote, &branch
		}
		run(protocol.GitAction{Kind: "pull", Data: input}, nil)
	case "git_push", "git_force_push", "git_push_to":
		var remote *string
		if a.Remote != "" {
			remote = &a.Remote
		}
		run(protocol.GitAction{Kind: "push", Data: protocol.GitActionPushPayload{Remote: remote, Publish: remote != nil && p.branches.Sync.Upstream == nil, Force: a.Force || a.Kind == "git_force_push"}}, nil)
	case "git_fetch", "git_fetch_prune", "git_fetch_all", "git_fetch_all_prune":
		run(protocol.GitAction{Kind: "fetch", Data: protocol.GitActionFetchPayload{Prune: a.Kind == "git_fetch_prune" || a.Kind == "git_fetch_all_prune", All: a.Kind == "git_fetch_all" || a.Kind == "git_fetch_all_prune"}}, nil)
	case "git_add_remote":
		p.panel.Prompt(a.Kind, []string{"settings_name", "mcp_url"}, []string{"", ""}, func(v []string, done func(bool)) {
			run(protocol.GitAction{Kind: "add_remote", Data: protocol.GitActionAddRemotePayload{Name: v[0], Url: v[1]}}, done)
		})
	case "git_remove_remote":
		run(protocol.GitAction{Kind: "remove_remote", Data: protocol.GitActionRemoveRemotePayload{Name: a.Remote}}, nil)
	case "branch_create", "branch_rename":
		initial := ""
		if a.Kind == "branch_rename" {
			initial = a.Target
		}
		p.panel.Prompt(map[bool]string{true: "git_rename_branch", false: "git_new_branch"}[a.Kind == "branch_rename"], []string{"settings_name"}, []string{initial}, func(v []string, done func(bool)) {
			var command protocol.Command
			kind := gitCreate
			commit := a.Secondary
			if commit == "" {
				commit = workspaceString(status.Head)
			}
			if a.Kind == "branch_rename" {
				kind = gitRename
				command = protocol.Command{Kind: "rename_git_branch", Data: protocol.CommandRenameGitBranchPayload{Worktree: p.owner.ID, Name: a.Target, NewName: v[0], Commit: commit}}
			} else if commit == "" {
				runAs(gitCreate, protocol.GitAction{Kind: "start_branch", Data: protocol.GitActionStartBranchPayload{Name: v[0]}}, done)
				return
			} else {
				command = protocol.Command{Kind: "create_git_branch", Data: protocol.CommandCreateGitBranchPayload{Worktree: p.owner.ID, Name: v[0], Commit: commit}}
			}
			p.mutateNamed(kind, command, func(c *ui.Context, out protocol.Output) {
				done(out.Kind != "")
				if out.Kind != "" {
					kit.Toast(c, kit.Done, i18n.T(c, "workspace_completed"))
				}
			})
		})
	case "branch_switch":
		p.switchBranch(a, status)
	case "branch_delete":
		p.mutateNamed(gitDelete, protocol.Command{Kind: "delete_git_branch", Data: protocol.CommandDeleteGitBranchPayload{Worktree: p.owner.ID, Name: a.Target, Commit: a.Secondary, ExpectedHead: workspaceString(status.Head), ExpectedBranch: status.Branch}}, nil)
	case "branch_merge":
		for _, branch := range p.branches.Entries {
			if branch.Name == a.Target && branch.Remote {
				runAs(gitMerge, protocol.GitAction{Kind: "merge", Data: protocol.GitActionMergePayload{Commit: a.Secondary}}, nil)
				return
			}
		}
		p.mutateNamed(gitMerge, protocol.Command{Kind: "merge_git_branch", Data: protocol.CommandMergeGitBranchPayload{Worktree: p.owner.ID, Name: a.Target, Commit: a.Secondary, ExpectedHead: workspaceString(status.Head), ExpectedBranch: workspaceString(status.Branch), ExpectedIndex: workspaceIndex(status)}}, nil)
	case "branch_rebase":
		p.panel.Confirm("git_rebase", a.Target, func(done func(bool)) {
			run(protocol.GitAction{Kind: "rebase", Data: protocol.GitActionRebasePayload{Commit: a.Secondary}}, done)
		}, "git_delete_confirm")
	case "git_continue":
		run(protocol.GitAction{Kind: "continue"}, nil)
	case "git_initialize":
		execute := func(done func(bool)) { run(protocol.GitAction{Kind: "initialize"}, done) }
		if len(status.Repositories) > 0 {
			p.panel.Confirm("git_initialize", strings.Join(status.Repositories, "\n"), execute, "git_initialize_nested_confirm")
		} else {
			execute(nil)
		}
	case "git_delete_remote_branch":
		run(protocol.GitAction{Kind: "delete_remote_branch", Data: protocol.GitActionDeleteRemoteBranchPayload{Branch: a.Target, Commit: a.Secondary}}, nil)
	case "git_stash_tracked", "git_stash_all", "git_stash_staged":
		mode := protocol.GitStashModeTracked
		if a.Kind == "git_stash_all" {
			mode = protocol.GitStashModeAll
		}
		if a.Kind == "git_stash_staged" {
			mode = protocol.GitStashModeStaged
		}
		run(protocol.GitAction{Kind: "stash", Data: protocol.GitActionStashPayload{Mode: mode}}, nil)
	case "git_stash_apply_latest", "git_stash_pop_latest", "git_stash_apply", "git_stash_pop":
		commit := a.Target
		if strings.HasSuffix(a.Kind, "_latest") && len(p.branches.Sync.Stashes) > 0 {
			commit = p.branches.Sync.Stashes[0].Commit
		}
		run(protocol.GitAction{Kind: "apply_stash", Data: protocol.GitActionApplyStashPayload{Commit: commit, Pop: strings.Contains(a.Kind, "pop")}}, nil)
	case "git_stash_drop":
		run(protocol.GitAction{Kind: "drop_stash", Data: protocol.GitActionDropStashPayload{Commit: a.Target}}, nil)
	case "git_stash_drop_all":
		commits := append([]string{}, a.Paths...)
		run(protocol.GitAction{Kind: "drop_all_stashes", Data: protocol.GitActionDropAllStashesPayload{Commits: commits}}, nil)
	case "git_stash_view":
		p.openGitStash(a.Target)
	case "git_tag_create":
		commit := workspaceString(status.Head)
		p.panel.Prompt(a.Kind, []string{"settings_name", "git_commit_message"}, []string{"", ""}, func(v []string, done func(bool)) {
			var message *string
			if v[1] != "" {
				message = &v[1]
			}
			run(protocol.GitAction{Kind: "create_tag", Data: protocol.GitActionCreateTagPayload{Name: v[0], Commit: commit, Message: message}}, done)
		})
	case "git_tag_delete":
		run(protocol.GitAction{Kind: "delete_tag", Data: protocol.GitActionDeleteTagPayload{Name: a.Target, Commit: a.Secondary}}, nil)
	case "git_tag_delete_remote":
		p.readRemoteTags(a.Remote, func(c *ui.Context, tags []protocol.GitTag) {
			choices := make([]panel.Choice, 0, len(tags))
			for _, tag := range tags {
				tag := tag
				choices = append(choices, panel.Choice{Label: tag.Name, Detail: tag.Commit, Run: func(c *ui.Context) {
					p.panel.Confirm("git_tag_delete_remote", a.Remote+"/"+tag.Name, func(done func(bool)) {
						run(protocol.GitAction{Kind: "delete_tag", Data: protocol.GitActionDeleteTagPayload{Name: tag.Name, Commit: tag.Commit, Remote: &a.Remote}}, done)
					})
				}})
			}
			p.panel.Choose(i18n.T(c, "git_tag_delete_remote"), choices)
		})
	case "git_tag_push":
		run(protocol.GitAction{Kind: "push_tags", Data: protocol.GitActionPushTagsPayload{Remote: a.Remote}}, nil)
	case "worktree_create":
		p.createWorktree(c, a, status)
	case "worktree_manage":
		p.manageWorktrees()
	case "git_output":
		p.openGitOutput()
	case "git_clone":
		p.runtime.App.Sidebar.OpenCloneEditor()
	default:
		p.workspaceError(c, protocol.ErrorCodeInvalidRequest, "unknown Git action")
	}
}
func workspaceCommitScope(status protocol.GitStatus, scope string) (tracked, all bool) {
	if scope == "all" {
		return false, true
	}
	if scope == "staged" {
		return false, false
	}
	for _, entry := range status.Entries {
		if entry.Staged != nil {
			return false, false
		}
	}
	return true, false
}
func (p *workspaceProject) readRemoteTags(remote string, done func(*ui.Context, []protocol.GitTag)) {
	p.read("remote-tags/"+remote, protocol.Command{Kind: "list_git_remote_tags", Data: protocol.CommandListGitRemoteTagsPayload{Worktree: p.owner.ID, Remote: remote}}, func(c *ui.Context, out protocol.Output) {
		if value, ok := detail[[]protocol.GitTag](out.Data); ok {
			done(c, value)
		}
	})
}
