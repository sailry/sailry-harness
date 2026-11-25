package native

import (
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/sidebar"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

type ProjectWorkflow = shell.ProjectWorkflow
type ProjectWorkflowHost = shell.ProjectWorkflowHost
type ProjectMenuAction = sidebar.ProjectMenuAction
type ProjectMenuProject = viewmodel.Project
type ProjectOverflowMenu = kit.Menu
type ProjectLocationReader = shell.ProjectLocationReader
type ConversationWorktreeActions = shell.ConversationWorktreeActions
type WorkflowConversation = shell.WorkflowConversation
type WorkflowWorkspace = shell.WorkflowWorkspace
type WorkspacePromptToggle = panel.PromptToggle
