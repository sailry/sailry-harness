package shell

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/sidebar"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// ProjectWorkflow binds an optional workflow to one live window.
type ProjectWorkflow func(ProjectWorkflowHost)

type ProjectWorkflowHost struct {
	RegisterMenu            func(sidebar.ProjectMenuAction)
	RegisterLocations       func(func(ProjectLocationReader))
	RegisterWorktreeActions func(ConversationWorktreeActions)
	NewDraft                func(*viewmodel.Project) (WorkflowConversation, bool)
	OpenLocation            sidebar.ProjectOpen
}

// ProjectLocationReader reads and publishes the visible checkouts of one project.
// The host owns the cache and invalidates it when that workspace changes.
type ProjectLocationReader struct {
	Main    protocol.WorktreeID
	Read    func(string, protocol.Command, func(protocol.Output))
	Publish func(protocol.GitWorktrees)
}

type ConversationWorktreeActions struct {
	Create, ForkTurn, ForkSession func(WorkflowConversation)
}

// WorkflowConversation captures the Node and source of an explicitly invoked action.
type WorkflowConversation struct {
	Session       *protocol.Session
	Execute       func(string, protocol.Command, func(*ui.Context, protocol.Output, error))
	Report        func(*ui.Context, error, string, string, string)
	OpenWorkspace func() (WorkflowWorkspace, bool)
}

// WorkflowWorkspace exposes the shared workspace prompt, request lane and
// location projection without choosing the workflow's commands or policy.
type WorkflowWorkspace struct {
	Worktree        protocol.Worktree
	Read            func(string, protocol.Command, func(*ui.Context, protocol.Output))
	Prompt          func(string, []string, []string, func([]string, func(bool)), ...panel.PromptToggle)
	LockPrompt      func(bool)
	ReportMutation  func(*ui.Context, string, string, error) bool
	Admit           func(protocol.Worktree)
	UseLocation     func(protocol.Worktree)
	OpenSession     func(protocol.SessionID)
	Refresh         func()
	ManageLocations func()
}
