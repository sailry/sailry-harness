package live

import (
	"encoding/json"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
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
		// The draft's row may sit under another Host's project: that project
		// is not this Host's.
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
func (r *Runtime) projectWorkspaceComposer(cv *viewmodel.Conversation, comp *composer.State, ctx *composer.Context) {
	if r.workspace == nil {
		r.initWorkspace()
	}
	worktreeActions := r.localRuntime().worktreeActions
	ctx.NoWorktreeCreate = worktreeActions.Create == nil
	r.projectWorkspaceLocations()
	session, ok := r.session(cv.ID)
	if !ok {
		session, ok = r.draftWorkspaceSession(cv.ID)
	}
	var p *workspaceProject
	if ok {
		if worktree, found := r.workspaceWorktree(session.Worktree); found {
			p = r.workspaceFor(worktree)
		}
	}
	if p == nil {
		ctx.Repository = composer.Repository{Kind: composer.Unavailable}
		comp.ActivityCallbacks = nil
		return
	}
	if r.controller.Connected() {
		if !p.statusKnown {
			p.inspectGit()
		}
		if session.ID != (protocol.SessionID{}) {
			if state, observed := r.conversationState(session.ID); observed && state.Connected {
				p.readPageResources(state.Snapshot.Page)
			}
		}
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
	if p.repositoryUnavailable || p.git.Inspect == nil {
		ctx.Repository.Kind = composer.Unavailable
	}
	if p.statusKnown {
		kind := composer.Git
		if p.status.Kind == protocol.RepositoryKindDirectory {
			kind = composer.Directory
		}
		ctx.Repository = composer.Repository{Kind: kind, Branch: branchLabel(p.status.Branch, p.status.Head)}
		if p.git.Badge != nil {
			ctx.Activity.ResourceBadge = p.git.Badge()
		} else if p.git.Changes != nil {
			ctx.Activity.Changes = p.git.Changes()
		}
		if kind == composer.Git && p.git.Checkout != nil && session.Project != nil {
			if ctx.NoWorktreeCreate {
				ctx.PackageBranch = true
				ctx.CheckoutControl = func(c *ui.Context, inMenu bool) {
					p.git.Checkout(c, inMenu, r.gitComposerContext(cv.ID, comp, session, p, !ctx.Readonly))
				}
			} else if worktreeActions.BranchOptions != nil && session.ID != (protocol.SessionID{}) {
				ctx.WorktreeOptions = func(c *ui.Context, menu *kit.Menu) {
					git := r.gitComposerContext(cv.ID, comp, session, p, !ctx.Readonly)
					if git.ForkBase != nil && git.Enabled {
						worktreeActions.BranchOptions(c, menu, p.data.Repo.Branches, git.ForkBase)
					}
				}
			}
		}
	}
	id := cv.ID
	comp.ActivityCallbacks = &composer.ActivityCallbacks{WorktreeEnabled: func() bool { return r.canMoveWorkspace(id, comp) }, Worktree: func(c *ui.Context, path string) {
		workspace := p.workflowConversationWorkspace(id, comp)
		if selectLocation := r.localRuntime().worktreeActions.Select; selectLocation != nil {
			selectLocation(workspace, c, path)
		} else if tree, ok := p.registeredLocation(path); ok && workspace.CanSelectLocation() {
			workspace.SelectLocation(c, tree)
		}
	}, WorktreeLabel: func(relative string) string {
		for _, project := range r.App.Data.Projects {
			if project.ID != comp.Project || project.Host != "" && project.Host != r.controller.Target().String() {
				continue
			}
			for _, tree := range project.Worktrees {
				if tree.Path == relative && tree.Branch != "" {
					return tree.Branch
				}
			}
		}
		return relative
	}}
	checkpoints := p.checkpointSessions[session.ID]
	if checkpoints == nil {
		return
	}
	for index := range cv.Turns {
		turn := &cv.Turns[index]
		changes, loaded := checkpoints.changes[turn.ID]
		if !loaded {
			turn.Reply.Changes = nil
			continue
		}
		turn.Reply.Changes = &conversation.Changes{Files: changes, Undone: checkpoints.undone(turn.ID, p.owner.ID)}
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
	readLocations := r.localRuntime().projectLocations
	for _, tree := range snapshot.Worktrees {
		delete(r.workspace.admitted, tree.ID)
	}
	for _, project := range r.App.Data.Projects {
		if project.Host != "" && project.Host != snapshot.Node.String() {
			continue
		}
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
		if readLocations != nil && !main.locationsLoaded {
			main.locationsLoaded = true
			readLocations(main.projectLocationReader())
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

// workspaceTurnReview retains the user's latest navigation intent while a
// turn diff loads. Repeating a pending turn may select another file of it.
type workspaceTurnReview struct {
	turn protocol.TurnID
	path string
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
	p.reviewTarget = nil
	p.reviewTurns = map[string]protocol.TurnID{}
	p.resetWorkingChanges()
	p.panel.DiffOpen(relative, "all")
	r.App.RevealConversationPanel()
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
	p.reviewTarget = &workspaceTurnReview{turn: id, path: path}
	p.read("review-turn/"+session.ID.String()+"/"+turn, protocol.Command{Kind: "read_turn_diff", Data: protocol.CommandReadTurnDiffPayload{Session: session.ID, Turn: id}}, func(c *ui.Context, out protocol.Output) {
		value, ok := detail[protocol.ConversationCheckpointTurnDiff](out.Data)
		if !ok || p.session != session.ID || r.App.Sidebar.Selected != session.ID.String() || r.selectedRuntime() != r || p.reviewTarget == nil || p.reviewTarget.turn != id {
			return
		}
		changes := workspaceTurnFiles(value)
		p.sessionCheckpoints(session.ID).recordChanges(turn, workspaceTurnSummary(value))
		p.turnView = &id
		p.data.Changes = changes
		for _, file := range changes {
			p.reviewTurns[file.Path] = id
		}
		target := p.reviewTarget.path
		if target == "" {
			target = ":changes"
		}
		if target != ":changes" {
			p.panel.DiffOpen(target, "all")
		} else {
			p.panel.Git.Show(target)
		}
		r.App.RevealConversationPanel()
		r.App.Panel.Show(panel.Tab{Kind: panel.Changes, ID: target})
	})
}

// The transcript keeps only the path and totals. The review tab owns diff lines.
func workspaceTurnSummary(value protocol.ConversationCheckpointTurnDiff) []conversation.ChangedFile {
	changes := make([]conversation.ChangedFile, 0, len(value.Files))
	for _, file := range value.Files {
		changes = append(changes, conversation.ChangedFile{Path: file.Path, Added: int(file.Additions), Removed: int(file.Deletions)})
	}
	return changes
}
func workspaceTurnFiles(value protocol.ConversationCheckpointTurnDiff) []viewmodel.FileChange {
	changes := make([]viewmodel.FileChange, 0, len(value.Files))
	for _, file := range value.Files {
		change := viewmodel.FileChange{
			Path: file.Path, Status: "M", Unstaged: "M", Hunks: viewmodel.ParseDiff(file.Text),
			Diff:  viewmodel.DiffState{Scope: "all", Binary: file.Binary, Partial: file.Truncated},
			Added: int(file.Additions), Removed: int(file.Deletions),
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
		r.App.RevealConversationPanel()
		r.App.Panel.Show(panel.Tab{Kind: panel.History})
		p.panel.Confirm("turn_changes_undo_all", target, func(done func(bool)) { p.restoreTurn(session.ID, turn, relative, done) }, "workspace_restore_confirm")
	})
}

// undone reads the card's state from the Node's checkpoint list: a file is
// undone once every change the turn made to it was restored, and the turn
// (the empty path) once every file is. It is the same after a restart, a
// session switch and on another controller.
func (c *workspaceCheckpoints) undone(turn string, worktree protocol.WorktreeID) map[string]bool {
	var out map[string]bool
	all := false
	for _, file := range c.files[turn] {
		if file.Worktree != worktree {
			continue
		}
		if out == nil {
			out, all = map[string]bool{}, true
		}
		if restored, seen := out[file.Path]; !seen || restored {
			out[file.Path] = file.Restored
		}
	}
	for path, restored := range out {
		if !restored {
			delete(out, path)
			all = false
		}
	}
	if all {
		out[""] = true
	}
	return out
}
func (p *workspaceProject) restoreTurn(session protocol.SessionID, turn, relative string, done func(bool)) {
	checkpoints := p.checkpointSessions[session]
	commands := []protocol.Command{}
	if checkpoints != nil {
		for _, file := range checkpoints.files[turn] {
			if file.Worktree == p.owner.ID && (relative == "" || file.Path == relative) {
				commands = append(commands, protocol.Command{Kind: "restore_file_checkpoint", Data: protocol.CommandRestoreFileCheckpointPayload{Session: session, Checkpoint: file.ID, Worktree: p.owner.ID}})
			}
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
		// The Node answered every restore; its list says the same when read again.
		if current := p.checkpointSessions[session]; current != nil {
			for i := range current.files[turn] {
				file := &current.files[turn][i]
				if file.Worktree == p.owner.ID && (relative == "" || file.Path == relative) {
					file.Restored = true
				}
			}
		}
		p.refresh()
	})
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
