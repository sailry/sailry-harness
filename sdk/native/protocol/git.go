package protocol

import "github.com/sailry/sailry-harness/internal/protocol"

type (
	GitBranch                          = protocol.GitBranch
	GitLogEntry                        = protocol.GitLogEntry
	GitLineStats                       = protocol.GitLineStats
	GitSync                            = protocol.GitSync
	CommandReadFilePayload             = protocol.CommandReadFilePayload
	FileContent                        = protocol.FileContent
	GitActionAddRemotePayload          = protocol.GitActionAddRemotePayload
	GitActionApplyStashPayload         = protocol.GitActionApplyStashPayload
	GitActionCherryPickPayload         = protocol.GitActionCherryPickPayload
	GitActionCreateTagPayload          = protocol.GitActionCreateTagPayload
	GitActionDeleteRemoteBranchPayload = protocol.GitActionDeleteRemoteBranchPayload
	GitActionDeleteTagPayload          = protocol.GitActionDeleteTagPayload
	GitActionDiscardAllPayload         = protocol.GitActionDiscardAllPayload
	GitActionDiscardPayload            = protocol.GitActionDiscardPayload
	GitActionDropAllStashesPayload     = protocol.GitActionDropAllStashesPayload
	GitActionDropStashPayload          = protocol.GitActionDropStashPayload
	GitActionFetchPayload              = protocol.GitActionFetchPayload
	GitActionIgnorePayload             = protocol.GitActionIgnorePayload
	GitActionMergePayload              = protocol.GitActionMergePayload
	GitActionPullPayload               = protocol.GitActionPullPayload
	GitActionPushPayload               = protocol.GitActionPushPayload
	GitActionPushTagsPayload           = protocol.GitActionPushTagsPayload
	GitActionRebasePayload             = protocol.GitActionRebasePayload
	GitActionRemoveRemotePayload       = protocol.GitActionRemoveRemotePayload
	GitActionStartBranchPayload        = protocol.GitActionStartBranchPayload
	GitActionStashPayload              = protocol.GitActionStashPayload
	GitActionSwitchStashingPayload     = protocol.GitActionSwitchStashingPayload
	GitActionSwitchTrackingPayload     = protocol.GitActionSwitchTrackingPayload
	GitActionSyncPayload               = protocol.GitActionSyncPayload
	GitAfterCommit                     = protocol.GitAfterCommit
	GitBranches                        = protocol.GitBranches
	GitChangeKind                      = protocol.GitChangeKind
	GitCommit                          = protocol.GitCommit
	GitCommitOptions                   = protocol.GitCommitOptions
	GitDiff                            = protocol.GitDiff
	GitDiffScope                       = protocol.GitDiffScope
	GitEntry                           = protocol.GitEntry
	GitHunkApplied                     = protocol.GitHunkApplied
	GitHunkOperation                   = protocol.GitHunkOperation
	GitLog                             = protocol.GitLog
	GitLogCursor                       = protocol.GitLogCursor
	GitTag                             = protocol.GitTag
	OutputGitCommitCreatedPayload      = protocol.OutputGitCommitCreatedPayload
)

const (
	GitFaultBranchExists     = protocol.GitFaultBranchExists
	GitFaultBranchInWorktree = protocol.GitFaultBranchInWorktree
	GitChangeKindModified    = protocol.GitChangeKindModified
	RepositoryKindReady      = protocol.RepositoryKindReady
	GitAfterCommitNone       = protocol.GitAfterCommitNone
	GitChangeKindAdded       = protocol.GitChangeKindAdded
	GitChangeKindDeleted     = protocol.GitChangeKindDeleted
	GitChangeKindRenamed     = protocol.GitChangeKindRenamed
	GitChangeKindTypeChanged = protocol.GitChangeKindTypeChanged
	GitDiffScopeAll          = protocol.GitDiffScopeAll
	GitIndexChangeStage      = protocol.GitIndexChangeStage
	GitIndexChangeUnstage    = protocol.GitIndexChangeUnstage
	GitStashModeAll          = protocol.GitStashModeAll
	GitStashModeStaged       = protocol.GitStashModeStaged
	GitStashModeTracked      = protocol.GitStashModeTracked
)

func DataAs[T any](data any) (T, bool) { return protocol.DataAs[T](data) }
