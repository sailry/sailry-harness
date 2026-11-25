package protocol

import "github.com/sailry/sailry-harness/internal/protocol"

type (
	CommandCreateManagedWorktreePayload = protocol.CommandCreateManagedWorktreePayload
	CommandForkConversationAtPayload    = protocol.CommandForkConversationAtPayload
	CommandInspectGitPayload            = protocol.CommandInspectGitPayload
	CommandListWorktreesPayload         = protocol.CommandListWorktreesPayload
	CommandMoveConversationPayload      = protocol.CommandMoveConversationPayload
	CommandReadConversationPayload      = protocol.CommandReadConversationPayload
	ConversationHistory                 = protocol.ConversationHistory
	ConversationPage                    = protocol.ConversationPage
	GitStatus                           = protocol.GitStatus
	GitWorktrees                        = protocol.GitWorktrees
	Session                             = protocol.Session
	SessionID                           = protocol.SessionID
	Worktree                            = protocol.Worktree
)

const (
	ConversationStatusQueued   = protocol.ConversationStatusQueued
	ConversationStatusRunning  = protocol.ConversationStatusRunning
	ConversationStatusStopping = protocol.ConversationStatusStopping
)

func NewWorktreeID() WorktreeID { return protocol.NewWorktreeID() }
