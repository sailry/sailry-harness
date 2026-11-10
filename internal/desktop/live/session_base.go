package live

import (
	"strings"
	"unicode"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// sessionBase is a new conversation's starting branch. The first send or
// attachment creates a managed worktree on Branch from Commit; the bound
// checkout itself is never switched.
type sessionBase struct {
	From   string `json:"from"`
	Branch string `json:"branch"`
	Commit string `json:"commit"`
}

func (base sessionBase) valid() bool {
	name := func(value string) bool {
		return value != "" && len(value) <= 1024 && !strings.ContainsFunc(value, unicode.IsControl)
	}
	return name(base.From) && name(base.Branch) && len(base.Commit) == 40 &&
		!strings.ContainsFunc(base.Commit, func(char rune) bool { return !strings.ContainsRune("0123456789abcdefABCDEF", char) })
}

// sessionBaseStart retains one draft's chosen base with the location it was
// chosen for, and coalesces every caller waiting for its worktree.
type sessionBaseStart struct {
	base     sessionBase
	project  protocol.ProjectID
	source   protocol.WorktreeID
	worktree string
	waiters  []func(*ui.Context, error)
}

// pendingSessionBase returns the draft's base while its location is unchanged.
func (r *Runtime) pendingSessionBase(comp *composer.State) *sessionBaseStart {
	start := r.sessionBases[comp]
	if start != nil && (comp.Project != start.project.String() || comp.Worktree != start.worktree) {
		delete(r.sessionBases, comp)
		return nil
	}
	return start
}

// startSessionBase creates the base worktree and moves the draft into it. An
// uncertain creation keeps the base, so a retry reuses the captured request.
func (x *conversationActions) startSessionBase(start *sessionBaseStart, done func(*ui.Context, error)) {
	r, before := x.r, x.cv.ID
	start.waiters = append(start.waiters, done)
	if len(start.waiters) > 1 {
		return
	}
	r.conversationCreating[before] = true
	r.executeScoped("session-base/"+before, protocol.Command{Kind: "create_managed_worktree", Data: protocol.CommandCreateManagedWorktreePayload{
		Project: start.project, Source: start.source, Branch: start.base.Branch, Base: &start.base.Commit,
	}}, func(c *ui.Context, output protocol.Output, err error) {
		delete(r.conversationCreating, before)
		waiters := start.waiters
		start.waiters = nil
		if err == nil {
			tree, valid := detail[protocol.Worktree](output.Data)
			switch {
			case !valid:
				err = fileActionFault(protocol.ErrorCodeOutcomeUnknown, "session base result is unavailable")
			case tree.Project == nil || *tree.Project != start.project || r.pendingSessionBase(x.comp) != start:
				err = fileActionFault(protocol.ErrorCodeConflict, "conversation location changed")
			default:
				if r.workspace == nil {
					r.initWorkspace()
				}
				r.workspace.admitted[tree.ID] = tree
				r.workspaceFor(tree)
				delete(r.sessionBases, x.comp)
				x.comp.Worktree = tree.Path
			}
		}
		for _, waiter := range waiters {
			waiter(c, err)
		}
	})
}

// draftWorktree resolves the checkout a draft starts in, including a worktree
// this controller created before its snapshot reports it.
func (r *Runtime) draftWorktree(comp *composer.State, project *protocol.ProjectID) *protocol.WorktreeID {
	matches := func(tree protocol.Worktree) bool {
		return (project == nil && tree.Project == nil || project != nil && tree.Project != nil && *project == *tree.Project) &&
			(tree.Path == comp.Worktree || comp.Worktree == "" && tree.Main)
	}
	for _, tree := range r.controller.Snapshot().Worktrees {
		if matches(tree) {
			return &tree.ID
		}
	}
	if r.workspace != nil {
		for _, tree := range r.workspace.admitted {
			if matches(tree) {
				return &tree.ID
			}
		}
	}
	return nil
}
