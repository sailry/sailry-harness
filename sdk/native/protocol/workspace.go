package protocol

import "github.com/sailry/sailry-harness/internal/protocol"

type (
	CommandApplyGitHunkPayload          = protocol.CommandApplyGitHunkPayload
	CommandCreateGitBranchPayload       = protocol.CommandCreateGitBranchPayload
	CommandCreateGitCommitPayload       = protocol.CommandCreateGitCommitPayload
	CommandDeleteGitBranchPayload       = protocol.CommandDeleteGitBranchPayload
	CommandListGitBranchesPayload       = protocol.CommandListGitBranchesPayload
	CommandListGitChangesPayload        = protocol.CommandListGitChangesPayload
	CommandListGitRemoteTagsPayload     = protocol.CommandListGitRemoteTagsPayload
	CommandMergeGitBranchPayload        = protocol.CommandMergeGitBranchPayload
	CommandReadGitBlamePayload          = protocol.CommandReadGitBlamePayload
	CommandReadGitChangePayload         = protocol.CommandReadGitChangePayload
	CommandReadGitCommitPayload         = protocol.CommandReadGitCommitPayload
	CommandReadGitContextPayload        = protocol.CommandReadGitContextPayload
	CommandReadGitDiffPayload           = protocol.CommandReadGitDiffPayload
	CommandReadGitHeadPayload           = protocol.CommandReadGitHeadPayload
	CommandReadGitLogPayload            = protocol.CommandReadGitLogPayload
	CommandReadGitOutputPayload         = protocol.CommandReadGitOutputPayload
	CommandReadGitShowPayload           = protocol.CommandReadGitShowPayload
	CommandReadGitStashPayload          = protocol.CommandReadGitStashPayload
	CommandRenameGitBranchPayload       = protocol.CommandRenameGitBranchPayload
	CommandSwitchGitBranchPayload       = protocol.CommandSwitchGitBranchPayload
	CommandUpdateGitIndexPayload        = protocol.CommandUpdateGitIndexPayload
	ConversationCheckpointHunkApplied   = protocol.ConversationCheckpointHunkApplied
	ConversationCheckpointHunkSelection = protocol.ConversationCheckpointHunkSelection
	OutputGitBranchDeletedPayload       = protocol.OutputGitBranchDeletedPayload
	OutputGitOutputPayload              = protocol.OutputGitOutputPayload
	ProjectsSourceClonePayload          = protocol.ProjectsSourceClonePayload
	CommandCreateManagedWorktreePayload = protocol.CommandCreateManagedWorktreePayload
	CommandCreateWorktreePayload        = protocol.CommandCreateWorktreePayload
	CommandForkConversationAtPayload    = protocol.CommandForkConversationAtPayload
	CommandInspectGitPayload            = protocol.CommandInspectGitPayload
	CommandListWorktreesPayload         = protocol.CommandListWorktreesPayload
	CommandMoveConversationPayload      = protocol.CommandMoveConversationPayload
	CommandReadConversationPayload      = protocol.CommandReadConversationPayload
	CommandRegisterWorktreePayload      = protocol.CommandRegisterWorktreePayload
	CommandRemoveWorktreePayload        = protocol.CommandRemoveWorktreePayload
	CommandResolveGitRevisionPayload    = protocol.CommandResolveGitRevisionPayload
	CommandRunGitActionPayload          = protocol.CommandRunGitActionPayload
	ConversationHistory                 = protocol.ConversationHistory
	ConversationPage                    = protocol.ConversationPage
	GitAction                           = protocol.GitAction
	GitActionPruneWorktreePayload       = protocol.GitActionPruneWorktreePayload
	GitStatus                           = protocol.GitStatus
	GitWorktree                         = protocol.GitWorktree
	GitWorktrees                        = protocol.GitWorktrees
	OutputGitRevisionPayload            = protocol.OutputGitRevisionPayload
	OutputWorktreeRemovedPayload        = protocol.OutputWorktreeRemovedPayload
	ProjectID                           = protocol.ProjectID
	Session                             = protocol.Session
	SessionID                           = protocol.SessionID
	Worktree                            = protocol.Worktree
)

const (
	ErrorCodeWrongTarget       = protocol.ErrorCodeWrongTarget
	ConversationStatusQueued   = protocol.ConversationStatusQueued
	ConversationStatusRunning  = protocol.ConversationStatusRunning
	ConversationStatusStopping = protocol.ConversationStatusStopping
	RepositoryKindDirectory    = protocol.RepositoryKindDirectory
)

func NewWorktreeID() WorktreeID { return protocol.NewWorktreeID() }
