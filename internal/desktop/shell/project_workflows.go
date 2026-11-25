package shell

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
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
	RegisterWorkspaceAction func(string, WorkspaceAction)
	NewDraft                func(*viewmodel.Project) (WorkflowConversation, bool)
	ProjectWorkspace        func(*viewmodel.Project) (WorkflowWorkspace, bool)
}

type WorkspaceAction func(*ui.Context, WorkflowWorkspace, string, string)

// ProjectLocationReader reads and publishes the visible checkouts of one project.
// The host owns the cache and invalidates it when that workspace changes.
type ProjectLocationReader struct {
	Main    protocol.WorktreeID
	Read    func(string, protocol.Command, func(protocol.Output))
	Publish func(protocol.GitWorktrees)
}

type ConversationWorktreeActions struct {
	Create, ForkTurn, ForkSession func(WorkflowConversation)
	Select                        func(WorkflowWorkspace, *ui.Context, string)
	BranchOptions                 func(*ui.Context, *kit.Menu, []viewmodel.Branch, func(*ui.Context, string, string))
}

// WorkflowConversation captures the Node and source of an explicitly invoked action.
type WorkflowConversation struct {
	Session       *protocol.Session
	Execute       func(string, protocol.Command, func(*ui.Context, protocol.Output, error))
	Report        func(*ui.Context, error, string, string, string)
	OpenWorkspace func() (WorkflowWorkspace, bool)
}

// WorkflowWorkspace captures a workspace and the window surface that opened it.
// Current reports whether its controls and location selection are still active.
type WorkflowWorkspace struct {
	Worktree           protocol.Worktree
	Session            *protocol.Session
	Presentation       *panel.Project
	Current            func() bool
	Read               func(string, protocol.Command, func(*ui.Context, protocol.Output))
	Execute            func(string, protocol.Command, func(*ui.Context, protocol.Output, error))
	Report             func(*ui.Context, error, string, string, string)
	Pick               func(string, []panel.PickerRow)
	Prompt             func(string, []string, []string, func([]string, func(bool)), ...panel.PromptToggle)
	Confirm            func(string, string, func(func(bool)), ...string)
	LockPrompt         func(bool)
	SetPromptAction    func(string)
	SetPromptDetail    func(string, string)
	SetPromptUncertain func(bool)
	Locations          func() ([]protocol.Worktree, []protocol.Session)
	HasDrafts          func(protocol.WorktreeID) bool
	Admit              func(protocol.Worktree)
	Release            func(protocol.WorktreeID)
	UseLocation        func(protocol.Worktree)
	CanSelectLocation  func() bool
	SelectLocation     func(*ui.Context, protocol.Worktree)
	OpenSession        func(protocol.SessionID)
	Refresh            func()
	RefreshLocations   func()
}
