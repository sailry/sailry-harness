package live

import (
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// worktreeEntry is a worktree of the project's repository as the manager
// lists it: its label, its markers and what choosing it offers.
type worktreeEntry struct {
	tree  protocol.GitWorktree
	label string
	// marks are the locale keys of its markers: main, locked, unavailable.
	marks []string
	// current is the worktree the manager was opened from.
	current bool
	// stale is a registration whose directory is gone; choosing it offers
	// to clean the registration up.
	stale bool
	// open and remove are the actions an available worktree offers.
	open, remove bool
}

// worktreeEntries lists every worktree Git reports, the main one first.
// Removal is offered for a clean candidate only: a linked, unlocked worktree
// on a branch that is not the current one. The Node still refuses a worktree
// with uncommitted changes, as Git does.
func worktreeEntries(trees protocol.GitWorktrees, current string) []worktreeEntry {
	entries := make([]worktreeEntry, 0, len(trees.Entries))
	for _, tree := range trees.Entries {
		entry := worktreeEntry{tree: tree, label: worktreeLabel(tree), current: tree.Available && tree.Path == current}
		if tree.Main {
			entry.marks = append(entry.marks, "worktree_main")
		}
		if tree.Locked {
			entry.marks = append(entry.marks, "worktree_locked")
		}
		switch {
		case !tree.Available && !tree.Main && !tree.Locked:
			entry.stale = true
			entry.marks = append(entry.marks, "worktree_prune_on_select")
		case !tree.Available:
			entry.marks = append(entry.marks, "worktree_unavailable")
		default:
			entry.open = !entry.current
			entry.remove = !entry.current && !tree.Main && !tree.Locked && tree.Branch != nil && tree.Head != nil
		}
		entries = append(entries, entry)
	}
	return entries
}

// worktreeLabel is a worktree's branch, else its short commit, else its
// directory.
func worktreeLabel(tree protocol.GitWorktree) string {
	if branch := workspaceString(tree.Branch); branch != "" {
		return branch
	}
	if head := workspaceString(tree.Head); head != "" {
		return head[:min(8, len(head))]
	}
	return worktreeName(tree.Path)
}

// worktreeName is the last element of a Node path, whichever separator the
// Node's system uses.
func worktreeName(path string) string {
	trimmed := strings.TrimRight(path, `/\`)
	if index := strings.LastIndexAny(trimmed, `/\`); index >= 0 {
		return trimmed[index+1:]
	}
	if trimmed == "" {
		return path
	}
	return trimmed
}

// manageWorktrees opens the worktree manager: every worktree of the
// repository, the current one checked, and New worktree.
func (p *workspaceProject) manageWorktrees() {
	command := protocol.Command{Kind: "list_worktrees", Data: protocol.CommandListWorktreesPayload{Worktree: p.owner.ID}}
	p.runtime.executeScoped("workspace-read/"+p.owner.ID.String()+"/worktrees", command, func(c *ui.Context, out protocol.Output, err error) {
		trees, ok := detail[protocol.GitWorktrees](out.Data)
		switch {
		case err != nil || !ok:
			kit.Toast(c, kit.Failed, i18n.T(c, "worktree_load_failed"))
		case p.owner.Project == nil:
			kit.Toast(c, kit.Failed, i18n.T(c, "worktree_missing_project"))
		case trees.Kind == protocol.RepositoryKindDirectory:
			kit.Toast(c, kit.Failed, i18n.T(c, "worktree_no_repository"))
		default:
			p.panel.Pick(i18n.T(c, "worktree_manage"), p.worktreeRows(c, trees))
		}
	})
}

func (p *workspaceProject) worktreeRows(c *ui.Context, trees protocol.GitWorktrees) []panel.PickerRow {
	group := i18n.T(c, "workspace_worktrees")
	rows := []panel.PickerRow{}
	var base *protocol.GitWorktree
	unused := unusedWorktrees(p.runtime.snapshot(), p.owner.Project)
	for _, entry := range worktreeEntries(trees, p.owner.Path) {
		details := make([]string, 0, len(entry.marks)+2)
		for _, mark := range entry.marks {
			details = append(details, i18n.T(c, mark))
		}
		if unused[entry.tree.Path] && entry.tree.Available {
			details = append(details, i18n.T(c, "worktree_unused"))
		}
		details = append(details, worktreeName(entry.tree.Path))
		row := panel.PickerRow{Label: entry.label, Detail: strings.Join(details, " · "), Group: group, Icon: icons.GitBranch, Checked: entry.current}
		if entry.tree.Main {
			row.Icon = icons.Folder
		}
		switch {
		case entry.stale:
			row.Run = func(*ui.Context) { p.confirmWorktreePrune(entry.tree) }
		case entry.tree.Available:
			row.Run = func(c *ui.Context) { p.panel.Pick(entry.tree.Path, p.worktreeActions(c, entry)) }
		}
		rows = append(rows, row)
		if entry.tree.Available && entry.tree.Head != nil && (base == nil || entry.current) {
			base = &entry.tree
		}
	}
	if base != nil {
		start := panel.GitAction{Target: workspaceString(base.Branch), Secondary: workspaceString(base.Head)}
		rows = append(rows, panel.PickerRow{Label: i18n.T(c, "worktree_create"), Group: i18n.T(c, "worktree_actions"), Icon: icons.Plus,
			Run: func(c *ui.Context) { p.createWorktree(c, start, p.status) }})
	}
	if trees.Truncated || trees.OmittedPaths > 0 {
		rows = append(rows, panel.PickerRow{Label: i18n.T(c, "worktree_listing_partial"), Disabled: true})
	}
	return rows
}

// worktreeActions are what an available worktree offers; one that cannot be
// done is left out.
func (p *workspaceProject) worktreeActions(c *ui.Context, entry worktreeEntry) []panel.PickerRow {
	path := entry.tree.Path
	rows := []panel.PickerRow{}
	if entry.open {
		rows = append(rows, panel.PickerRow{Label: i18n.T(c, "workspace_open_worktree"), Icon: icons.Folder,
			Run: func(c *ui.Context) { p.openWorktree(c, path) }})
	}
	rows = append(rows, panel.PickerRow{Label: i18n.T(c, "workspace_copy_path"), Icon: icons.Copy,
		Run: func(c *ui.Context) { c.WriteClipboard(path) }})
	if entry.remove {
		rows = append(rows, panel.PickerRow{Label: i18n.T(c, "worktree_remove"), Icon: icons.Trash,
			Run: func(*ui.Context) { p.confirmWorktreeRemoval(entry.tree) }})
	}
	return rows
}

// openWorktree makes a worktree the working location of the conversation the
// manager was opened from: an unsent draft starts there and an idle session
// moves there. Both register a worktree the Node does not know yet.
func (p *workspaceProject) openWorktree(c *ui.Context, path string) {
	r := p.runtime
	id := r.App.Sidebar.Selected
	cv := r.App.Data.Find(id)
	if cv == nil || r.workspace.selected != p.owner.ID {
		kit.Toast(c, kit.Conflict, i18n.T(c, "worktree_context_changed"))
		return
	}
	comp := r.App.Composer(id)
	if _, exists := r.session(id); !exists {
		r.selectDraftWorktree(comp, path)
		return
	}
	if !r.canMoveWorkspace(cv, comp) {
		kit.Toast(c, kit.Failed, i18n.T(c, "worktree_open_busy"))
		return
	}
	r.selectConversationWorktree(cv, comp, path)
}

// unusedWorktrees are the paths of a project's linked worktrees that the
// Node knows and no session uses: one a removed session left with uncommitted
// work, or one made in the manager that never had a session.
func unusedWorktrees(snapshot protocol.Snapshot, project *protocol.ProjectID) map[string]bool {
	used := map[protocol.WorktreeID]bool{}
	for _, session := range snapshot.Sessions {
		used[session.Worktree] = true
	}
	unused := map[string]bool{}
	for _, tree := range snapshot.Worktrees {
		if !tree.Main && !used[tree.ID] && tree.Project != nil && project != nil && *tree.Project == *project {
			unused[tree.Path] = true
		}
	}
	return unused
}

// registeredWorktree is the Node's record of the project's worktree at path.
func (p *workspaceProject) registeredWorktree(path string) (protocol.Worktree, bool) {
	matches := func(tree protocol.Worktree) bool {
		return tree.Path == path && tree.Project != nil && p.owner.Project != nil && *tree.Project == *p.owner.Project
	}
	for _, tree := range p.runtime.snapshot().Worktrees {
		if matches(tree) {
			return tree, true
		}
	}
	for _, tree := range p.runtime.workspace.admitted {
		if matches(tree) {
			return tree, true
		}
	}
	return protocol.Worktree{}, false
}

// confirmWorktreeRemoval asks once, naming the directory, and removes the
// worktree. The Node removes a registered worktree only, so one Git reports
// and the Node does not know is registered first.
func (p *workspaceProject) confirmWorktreeRemoval(tree protocol.GitWorktree) {
	if p.owner.Project == nil {
		return
	}
	project := *p.owner.Project
	p.panel.Confirm("worktree_remove", tree.Path, func(done func(bool)) {
		remove := func(id protocol.WorktreeID) {
			command := protocol.Command{Kind: "remove_worktree", Data: protocol.CommandRemoveWorktreePayload{
				Worktree: id, ExpectedHead: workspaceString(tree.Head), ExpectedBranch: workspaceString(tree.Branch),
			}}
			p.mutateNamed(worktreeRemove, command, func(c *ui.Context, out protocol.Output) {
				done(out.Kind != "")
				if out.Kind != "" {
					delete(p.runtime.workspace.admitted, id)
					p.locationsLoaded = false
					kit.Toast(c, kit.Done, i18n.T(c, "worktree_removed"))
				}
			})
		}
		if registered, ok := p.registeredWorktree(tree.Path); ok {
			remove(registered.ID)
			return
		}
		command := protocol.Command{Kind: "register_worktree", Data: protocol.CommandRegisterWorktreePayload{Project: project, Path: tree.Path}}
		p.mutateNamed(worktreeOpen, command, func(c *ui.Context, out protocol.Output) {
			registered, ok := detail[protocol.Worktree](out.Data)
			if !ok {
				done(false)
				return
			}
			p.runtime.workspace.admitted[registered.ID] = registered
			remove(registered.ID)
		})
	}, "worktree_remove_confirm")
}

// confirmWorktreePrune asks once, naming the missing directory, and removes
// its stale registration from the repository. The Git state the action
// expects is read first, and the same command is kept while its outcome is
// uncertain so that Check result asks about it and does not run it again.
func (p *workspaceProject) confirmWorktreePrune(tree protocol.GitWorktree) {
	var retained *protocol.Command
	p.panel.Confirm("worktree_prune", tree.Path, func(done func(bool)) {
		run := func(command protocol.Command) {
			retained = &command
			p.mutateNamed(worktreePrune, command, func(c *ui.Context, out protocol.Output) {
				done(out.Kind != "")
				if out.Kind != "" {
					kit.Toast(c, kit.Done, i18n.T(c, "worktree_pruned"))
					p.manageWorktrees()
				}
			}, func(_ *ui.Context, err error) {
				if faultCode(err) != protocol.ErrorCodeOutcomeUnknown {
					retained = nil
				}
			})
		}
		if retained != nil {
			run(*retained)
			return
		}
		inspect := protocol.Command{Kind: "inspect_git", Data: protocol.CommandInspectGitPayload{Worktree: p.owner.ID}}
		p.runtime.executeScoped("workspace-read/"+p.owner.ID.String()+"/worktree-prune", inspect, func(c *ui.Context, out protocol.Output, err error) {
			status, ok := detail[protocol.GitStatus](out.Data)
			if err != nil || !ok {
				kit.Toast(c, kit.Failed, i18n.T(c, "worktree_prune_failed"))
				done(false)
				return
			}
			run(p.gitCommand(status, protocol.GitAction{Kind: "prune_worktree", Data: protocol.GitActionPruneWorktreePayload{Path: tree.Path}}))
		})
	}, "worktree_prune_confirm")
}

// createWorktree asks for a starting point, a new branch and a directory.
// The starting point is any revision the repository resolves: a branch, a
// tag or a commit. It starts as the branch the action names, else the
// current branch or commit.
func (p *workspaceProject) createWorktree(c *ui.Context, a panel.GitAction, status protocol.GitStatus, created ...func(protocol.Worktree)) {
	if p.owner.Project == nil {
		kit.Toast(c, kit.Failed, i18n.T(c, "worktree_missing_project"))
		return
	}
	project := *p.owner.Project
	base := a.Target
	for _, candidate := range []string{workspaceString(status.Branch), a.Secondary, workspaceString(status.Head)} {
		if base == "" {
			base = candidate
		}
	}
	// The command is kept while its outcome is uncertain: the starting point
	// may resolve to another commit by the time the result is checked.
	var retained *protocol.Command
	p.panel.Prompt("worktree_create", []string{"worktree_base", "git_new_branch", "worktree_path"}, []string{base, "", ""}, func(v []string, done func(bool)) {
		run := func(command protocol.Command) {
			retained = &command
			p.mutateNamed(worktreeCreate, command, func(c *ui.Context, out protocol.Output) {
				tree, ok := detail[protocol.Worktree](out.Data)
				done(ok)
				if !ok {
					return
				}
				p.runtime.workspace.admitted[tree.ID] = tree
				p.runtime.workspaceFor(tree)
				p.locationsLoaded = false
				for _, callback := range created {
					callback(tree)
				}
				kit.Toast(c, kit.Done, i18n.T(c, "worktree_created"))
			}, func(_ *ui.Context, err error) {
				if faultCode(err) != protocol.ErrorCodeOutcomeUnknown {
					retained = nil
				}
			})
		}
		if retained != nil {
			run(*retained)
			return
		}
		revision, branch, path := strings.TrimSpace(v[0]), strings.TrimSpace(v[1]), strings.TrimSpace(v[2])
		resolve := protocol.Command{Kind: "resolve_git_revision", Data: protocol.CommandResolveGitRevisionPayload{Worktree: p.owner.ID, Revision: revision}}
		p.runtime.executeScoped("workspace-read/"+p.owner.ID.String()+"/worktree-base", resolve, func(c *ui.Context, out protocol.Output, err error) {
			resolved, ok := detail[protocol.OutputGitRevisionPayload](out.Data)
			if err != nil || !ok {
				kit.Toast(c, kit.Failed, i18n.T(c, "worktree_revision_failed"))
				done(false)
				return
			}
			run(protocol.Command{Kind: "create_worktree", Data: protocol.CommandCreateWorktreePayload{Project: project, Path: path, Branch: branch, Commit: resolved.Commit}})
		})
	})
	p.panel.SetPromptAction("worktree_create")
}
