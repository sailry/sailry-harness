package live

import (
	"cmp"
	"encoding/json"
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (r *Runtime) draftWorkspaceSession(id string) (protocol.Session, bool) {
	if !strings.HasPrefix(id, "draft/") || r.pluginComposerView(id) == nil {
		return protocol.Session{}, false
	}
	comp := r.App.Composer(id)
	project := comp.Project
	if project == "" {
		// The draft's row may still sit under another Host's project after
		// the host control moved the draft here: that project is not this
		// Host's.
		if owner := r.App.Data.ProjectOf(id); owner != nil && (owner.Host == "" || owner.Host == r.controller.Target().String()) {
			project = owner.ID
			comp.Project = project
		}
	}
	if project == "" {
		return protocol.Session{}, false
	}
	var selected *protocol.Worktree
	for _, tree := range r.snapshot().Worktrees {
		if tree.Project == nil || tree.Project.String() != project {
			continue
		}
		if selected == nil && tree.Main {
			copy := tree
			selected = &copy
		}
		if tree.Path == comp.Worktree {
			copy := tree
			selected = &copy
			break
		}
	}
	for _, tree := range r.workspace.admitted {
		if tree.Project != nil && tree.Project.String() == project && tree.Path == comp.Worktree {
			copy := tree
			selected = &copy
			break
		}
	}
	if selected == nil {
		return protocol.Session{}, false
	}
	comp.Worktree = selected.Path
	return protocol.Session{Worktree: selected.ID, Project: selected.Project}, true
}
func workspaceChanges(status protocol.GitStatus) *composer.Changes {
	if status.Kind == protocol.RepositoryKindDirectory {
		return nil
	}
	value := &composer.Changes{Files: len(status.Entries), Partial: status.Truncated}
	entries := append([]protocol.GitEntry(nil), status.Entries...)
	slices.SortStableFunc(entries, func(a, b protocol.GitEntry) int {
		var am, bm uint64
		if a.ModifiedMs != nil {
			am = *a.ModifiedMs
		}
		if b.ModifiedMs != nil {
			bm = *b.ModifiedMs
		}
		return cmp.Or(cmp.Compare(bm, am), cmp.Compare(a.Path, b.Path))
	})
	for _, entry := range entries {
		value.Added += int(entry.Diff.Additions)
		value.Removed += int(entry.Diff.Deletions)
	}
	for _, entry := range entries[:min(len(entries), composer.RecentChanges)] {
		status := workspaceChange(entry.Unstaged)
		if status == "" {
			status = workspaceChange(entry.Staged)
		}
		if entry.Untracked {
			status = "A"
		}
		value.Recent = append(value.Recent, composer.ChangedFile{Path: entry.Path, Status: status, Added: int(entry.Diff.Additions), Removed: int(entry.Diff.Deletions)})
	}
	return value
}
func (r *Runtime) projectWorkspaceComposer(cv *viewmodel.Conversation, comp *composer.State, ctx *composer.Context) {
	r.workspaceFrame(nil)
	session, ok := r.session(cv.ID)
	if !ok {
		session, ok = r.draftWorkspaceSession(cv.ID)
	}
	var p *workspaceProject
	if ok {
		p = r.workspace.projects[session.Worktree]
	}
	if p == nil {
		ctx.Repository = composer.Repository{Kind: composer.Unavailable}
		return
	}
	for index := range ctx.Projects {
		for _, project := range r.App.Data.Projects {
			if project.ID == ctx.Projects[index].ID && project.Host == ctx.Projects[index].Host {
				ctx.Projects[index].Worktrees = nil
				for _, tree := range project.Worktrees {
					ctx.Projects[index].Worktrees = append(ctx.Projects[index].Worktrees, composer.Worktree{Branch: tree.Path, Main: tree.Main})
				}
			}
		}
	}
	ctx.Repository = composer.Repository{Kind: composer.Inspecting}
	if p.failedReads["git-status"] {
		ctx.Repository.Kind = composer.Unavailable
	}
	if p.statusKnown {
		kind := composer.Git
		if p.status.Kind == protocol.RepositoryKindDirectory {
			kind = composer.Directory
		}
		ctx.Repository = composer.Repository{Kind: kind, Branch: branchLabel(p.status.Branch, p.status.Head)}
		ctx.Activity.Changes = workspaceChanges(p.status)
	}
	comp.ActivityCallbacks = &composer.ActivityCallbacks{WorktreeEnabled: func() bool { return r.canMoveWorkspace(cv, comp) }, Worktree: func(relative string) {
		if _, exists := r.session(cv.ID); exists {
			r.selectConversationWorktree(cv, comp, relative)
		} else {
			r.selectDraftWorktree(comp, relative)
		}
	}, WorktreeLabel: func(relative string) string {
		for _, project := range r.App.Data.Projects {
			for _, tree := range project.Worktrees {
				if tree.Path == relative && tree.Branch != "" {
					return tree.Branch
				}
			}
		}
		return relative
	}}
	for index := range cv.Turns {
		turn := &cv.Turns[index]
		changes, loaded := p.turnChanges[turn.ID]
		if !loaded {
			continue
		}
		projected := &conversation.Changes{Undone: p.turnUndone[turn.ID]}
		for _, file := range changes {
			projected.Files = append(projected.Files, conversation.ChangedFile{Path: file.Path, Added: file.Added, Removed: file.Removed})
		}
		turn.Reply.Changes = projected
	}
}

// branchLabel is the branch, or the short head of a detached checkout.
func branchLabel(branch, head *string) string {
	if label := workspaceString(branch); label != "" {
		return label
	}
	value := workspaceString(head)
	return value[:min(7, len(value))]
}
func (r *Runtime) projectWorkspaceLocations() {
	snapshot := r.snapshot()
	for _, tree := range snapshot.Worktrees {
		delete(r.workspace.admitted, tree.ID)
	}
	for _, project := range r.App.Data.Projects {
		var main *workspaceProject
		for _, tree := range snapshot.Worktrees {
			if tree.Project != nil && tree.Project.String() == project.ID && tree.Main {
				main = r.workspaceFor(tree)
				break
			}
		}
		if main == nil {
			continue
		}
		for i := range project.Worktrees {
			if project.Worktrees[i].Branch == "" {
				project.Worktrees[i].Branch = project.Worktrees[i].Path
			}
		}
		if !main.locationsLoaded {
			main.locationsLoaded = true
			main.read("git-worktrees", protocol.Command{Kind: "list_worktrees", Data: protocol.CommandListWorktreesPayload{Worktree: main.owner.ID}}, func(c *ui.Context, out protocol.Output) {
				if value, ok := detail[protocol.GitWorktrees](out.Data); ok {
					main.worktrees = value
					r.projectWorkspaceLocations()
				}
			})
		}
		if len(main.worktrees.Entries) > 0 {
			project.Worktrees = nil
			for _, tree := range main.worktrees.Entries {
				label := branchLabel(tree.Branch, tree.Head)
				if label == "" {
					label = tree.Path
				}
				project.Worktrees = append(project.Worktrees, viewmodel.Worktree{Branch: label, Path: tree.Path, Main: tree.Main})
				if tree.Main {
					project.Branch = label
				}
			}
		}
		for _, tree := range project.Worktrees {
			for _, cv := range project.Conversations {
				if cv.WorktreePath == tree.Path {
					cv.Worktree = tree.Branch
				}
			}
		}
		for _, cached := range r.workspace.projects {
			if cached.owner.Project != nil && cached.owner.Project.String() == project.ID && cached.statusKnown {
				label := branchLabel(cached.status.Branch, cached.status.Head)
				for i := range project.Worktrees {
					if project.Worktrees[i].Path == cached.owner.Path {
						project.Worktrees[i].Branch = label
					}
				}
				if cached.owner.Main {
					project.Branch = label
				}
				for _, cv := range project.Conversations {
					if cv.WorktreePath == cached.owner.Path {
						cv.Worktree = label
					}
				}
			}
		}
	}
}
func (r *Runtime) openWorkspaceChange(x *conversationActions, relative string) {
	session, ok := r.session(x.cv.ID)
	if !ok {
		return
	}
	r.App.Sidebar.Selected = x.cv.ID
	r.workspaceFrame(nil)
	p := r.workspace.projects[session.Worktree]
	if p == nil {
		return
	}
	p.turnView = nil
	delete(p.reviewTurns, relative)
	p.projectStatus(p.status)
	p.panel.DiffOpen(relative, "all")
	r.App.PanelOpen = true
	r.App.Panel.Show(panel.Tab{Kind: panel.Changes, ID: relative})
}
func (r *Runtime) reviewWorkspaceTurn(x *conversationActions, turn, path string) {
	session, ok := r.session(x.cv.ID)
	if !ok {
		return
	}
	id, err := protocol.ParseTurnID(turn)
	if err != nil {
		return
	}
	r.App.Sidebar.Selected = x.cv.ID
	r.workspaceFrame(nil)
	p := r.workspace.projects[session.Worktree]
	if p == nil {
		return
	}
	p.read("review-turn/"+turn, protocol.Command{Kind: "read_turn_diff", Data: protocol.CommandReadTurnDiffPayload{Session: session.ID, Turn: id}}, func(c *ui.Context, out protocol.Output) {
		value, ok := detail[protocol.ConversationCheckpointTurnDiff](out.Data)
		if !ok || p.session != session.ID {
			return
		}
		changes := workspaceTurnFiles(value)
		p.turnChanges[turn] = changes
		p.turnView = &id
		for _, file := range value.Files {
			p.panel.SetGitDiffState(file.Path, "all", panel.GitDiffState{Binary: file.Binary, Partial: file.Truncated})
		}
		p.data.Changes = changes
		for _, file := range changes {
			p.reviewTurns[file.Path] = id
		}
		target := path
		if target == "" {
			target = ":changes"
		}
		if target != ":changes" {
			p.panel.DiffOpen(target, "all")
		} else {
			p.panel.Git.Show(target)
		}
		r.App.PanelOpen = true
		r.App.Panel.Show(panel.Tab{Kind: panel.Changes, ID: target})
	})
}
func workspaceTurnFiles(value protocol.ConversationCheckpointTurnDiff) []viewmodel.FileChange {
	changes := make([]viewmodel.FileChange, 0, len(value.Files))
	for _, file := range value.Files {
		change := viewmodel.FileChange{Path: file.Path, Status: "M", Unstaged: "M", Hunks: workspaceHunks(file.Text)}
		for _, hunk := range change.Hunks {
			for _, line := range hunk.Lines {
				if line.Kind == viewmodel.Added {
					change.Added++
				}
				if line.Kind == viewmodel.Removed {
					change.Removed++
				}
			}
		}
		changes = append(changes, change)
	}
	return changes
}
func (r *Runtime) undoWorkspaceTurn(x *conversationActions, turn, relative string) {
	session, ok := r.session(x.cv.ID)
	if !ok {
		return
	}
	r.App.Sidebar.Selected = x.cv.ID
	r.workspaceFrame(nil)
	p := r.workspace.projects[session.Worktree]
	if p == nil {
		return
	}
	target := relative
	if target == "" {
		target = x.cv.Title
	}
	r.enqueue(func(c *ui.Context) {
		// Opening History keeps the existing confirmation visible without inventing a new surface.
		r.App.PanelOpen = true
		r.App.Panel.Show(panel.Tab{Kind: panel.History})
		p.panel.Confirm("turn_changes_undo_all", target, func(done func(bool)) { p.restoreTurn(session.ID, turn, relative, done) }, "workspace_restore_confirm")
	})
}
func (p *workspaceProject) restoreTurn(session protocol.SessionID, turn, relative string, done func(bool)) {
	commands := []protocol.Command{}
	for _, file := range p.checkpointOrder[turn] {
		if file.Worktree == p.owner.ID && (relative == "" || file.Path == relative) {
			commands = append(commands, protocol.Command{Kind: "restore_file_checkpoint", Data: protocol.CommandRestoreFileCheckpointPayload{Session: session, Checkpoint: file.ID, Worktree: p.owner.ID}})
		}
	}
	if len(commands) == 0 {
		p.runtime.enqueue(func(c *ui.Context) {
			p.workspaceError(c, protocol.ErrorCodeNotFound, "turn checkpoints are unavailable")
		})
		done(false)
		return
	}
	// The Node answers a file that is already restored as restored, so an Undo
	// repeated after a failure, a session switch or a restart continues where
	// the earlier one stopped and then marks the card.
	encoded, err := json.Marshal(commands)
	if err != nil {
		done(false)
		return
	}
	key := string(encoded)
	batch := p.restoreBatches[key]
	if batch == nil {
		batch = &fileBatch{}
		if p.restoreBatches == nil {
			p.restoreBatches = map[string]*fileBatch{}
		}
		p.restoreBatches[key] = batch
	}
	p.fileCommandsFinish(batch, commands, done, func() {
		delete(p.restoreBatches, key)
		if p.turnUndone[turn] == nil {
			p.turnUndone[turn] = map[string]bool{}
		}
		p.turnUndone[turn][relative] = true
		p.refresh()
	})
}
func (r *Runtime) createComposerWorktree(x *conversationActions) {
	var captured *protocol.Session
	if session, ok := r.session(x.cv.ID); ok {
		captured = &session
	}
	r.openManagedConversation(x.cv, captured, false, func(tree protocol.Worktree) { x.comp.Worktree = tree.Path })
}
func (r *Runtime) chooseComposerWorktree(x *conversationActions, relative string) {
	if _, exists := r.session(x.cv.ID); exists {
		return
	}
	r.selectDraftWorktree(x.comp, relative)
}
func (r *Runtime) selectDraftWorktree(comp *composer.State, relative string) {
	for _, tree := range r.snapshot().Worktrees {
		if tree.Path == relative && tree.Project != nil && tree.Project.String() == comp.Project {
			comp.Worktree = relative
			return
		}
	}
	p := r.workspace.projects[r.workspace.selected]
	if p == nil || p.owner.Project == nil {
		return
	}
	comp.Worktree = p.owner.Path
	p.mutateNamed(worktreeOpen, protocol.Command{Kind: "register_worktree", Data: protocol.CommandRegisterWorktreePayload{Project: *p.owner.Project, Path: relative}}, func(c *ui.Context, out protocol.Output) {
		if tree, ok := detail[protocol.Worktree](out.Data); ok {
			r.workspace.admitted[tree.ID] = tree
			r.workspaceFor(tree)
			comp.Worktree = tree.Path
			p.locationsLoaded = false
		}
	})
}
func (r *Runtime) sidebarWorkspaceAction(project *viewmodel.Project, operation, relative string) {
	if project == nil {
		return
	}
	owner := r.projectOverviewRuntime(project)
	if owner == nil {
		r.Report(fileActionFault(protocol.ErrorCodeUnavailable, "the execution host is unavailable"))
		return
	}
	if owner != r {
		owner.sidebarWorkspaceAction(project, operation, relative)
		return
	}
	if operation == "select" {
		r.openProjectWorktree(project, relative)
		return
	}
	r.localRuntime().newDraft(project)
	comp := r.App.Composer(r.App.Sidebar.Selected)
	comp.Project = project.ID
	if relative != "" {
		comp.Worktree = relative
	}
	r.workspaceFrame(nil)
	p := r.workspace.projects[r.workspace.selected]
	if p == nil {
		return
	}
	r.App.PanelOpen = true
	r.App.Panel.Show(panel.Tab{Kind: panel.Changes, ID: ":changes"})
	if operation == "create" {
		if cv := r.App.Data.Find(r.App.Sidebar.Selected); cv != nil {
			r.openManagedConversation(cv, nil, false, func(tree protocol.Worktree) { comp.Worktree = tree.Path; p.locationsLoaded = false })
		}
	} else {
		p.manageWorktrees()
	}
}

func (r *Runtime) openProjectWorktree(project *viewmodel.Project, relative string) {
	if worktree, ok := r.projectWorktree(project, relative); ok {
		r.openProjectOverviewAt(project, worktree.Path)
		return
	}
	if worktree, ok := r.projectWorktree(project, ""); ok {
		workspace := r.workspaceFor(worktree)
		page, selected := r.App.Page, r.App.Sidebar.Selected
		node, projectID, overview := r.App.ProjectOwner()
		workspace.mutateNamed(worktreeOpen, protocol.Command{Kind: "register_worktree", Data: protocol.CommandRegisterWorktreePayload{
			Project: *worktree.Project, Path: relative,
		}}, func(c *ui.Context, out protocol.Output) {
			if registered, ok := detail[protocol.Worktree](out.Data); ok {
				r.workspace.admitted[registered.ID] = registered
				workspace.locationsLoaded = false
				currentNode, currentProject, currentOverview := r.App.ProjectOwner()
				if r.projectOverviewRuntime(project) == r && r.App.Page == page && r.App.Sidebar.Selected == selected &&
					currentNode == node && currentProject == projectID && currentOverview == overview {
					r.openProjectOverviewAt(project, registered.Path)
				}
			}
		})
		return
	}
	r.Report(fileActionFault(protocol.ErrorCodeUnavailable, "the project worktree is unavailable"))
}

func (r *Runtime) projectWorktree(project *viewmodel.Project, relative string) (protocol.Worktree, bool) {
	matches := func(worktree protocol.Worktree) bool {
		return worktree.Project != nil && worktree.Project.String() == project.ID &&
			(relative == "" && worktree.Main || relative != "" && worktree.Path == relative)
	}
	for _, worktree := range r.snapshot().Worktrees {
		if matches(worktree) {
			return worktree, true
		}
	}
	for _, worktree := range r.workspace.admitted {
		if matches(worktree) {
			return worktree, true
		}
	}
	return protocol.Worktree{}, false
}
