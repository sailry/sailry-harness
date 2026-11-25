package protocol

import "github.com/sailry/sailry-harness/internal/protocol"

type (
	GitActionSwitchRemotePayload   = protocol.GitActionSwitchRemotePayload
	GitActionTrackPayload          = protocol.GitActionTrackPayload
	GitActionTrashPayload          = protocol.GitActionTrashPayload
	GitBlame                       = protocol.GitBlame
	GitBlameLine                   = protocol.GitBlameLine
	GitChange                      = protocol.GitChange
	GitChangeEntry                 = protocol.GitChangeEntry
	GitChangeLine                  = protocol.GitChangeLine
	GitChangeSnapshot              = protocol.GitChangeSnapshot
	GitChangeState                 = protocol.GitChangeState
	GitChangeStateCommitPayload    = protocol.GitChangeStateCommitPayload
	GitChangeStateRangePayload     = protocol.GitChangeStateRangePayload
	GitChangeStateWorktreePayload  = protocol.GitChangeStateWorktreePayload
	GitChangeTarget                = protocol.GitChangeTarget
	GitChangeTargetCommitPayload   = protocol.GitChangeTargetCommitPayload
	GitChangeTargetRangePayload    = protocol.GitChangeTargetRangePayload
	GitChangeTargetWorktreePayload = protocol.GitChangeTargetWorktreePayload
	GitChanges                     = protocol.GitChanges
	GitChangesCursor               = protocol.GitChangesCursor
	GitContext                     = protocol.GitContext
	GitFile                        = protocol.GitFile
	GitHead                        = protocol.GitHead
	GitIndex                       = protocol.GitIndex
	GitIndexChange                 = protocol.GitIndexChange
	GitInspectionReason            = protocol.GitInspectionReason
	GitMerge                       = protocol.GitMerge
	GitStash                       = protocol.GitStash
	RepositoryKind                 = protocol.RepositoryKind
	UpdateFilesChangedPayload      = protocol.UpdateFilesChangedPayload
)

const (
	GitAfterCommitPush                  = protocol.GitAfterCommitPush
	GitAfterCommitSync                  = protocol.GitAfterCommitSync
	GitBranchExists                     = protocol.GitBranchExists
	GitBranchInWorktree                 = protocol.GitBranchInWorktree
	GitChangeLineKindAdded              = protocol.GitChangeLineKindAdded
	GitChangeLineKindContext            = protocol.GitChangeLineKindContext
	GitChangeLineKindDeleted            = protocol.GitChangeLineKindDeleted
	GitContextSideNew                   = protocol.GitContextSideNew
	GitContextSideOld                   = protocol.GitContextSideOld
	GitDiffScopeStaged                  = protocol.GitDiffScopeStaged
	GitDiffScopeUnstaged                = protocol.GitDiffScopeUnstaged
	GitHunkReject                       = protocol.GitHunkReject
	GitHunkStage                        = protocol.GitHunkStage
	GitInspectionDeadlineExceeded       = protocol.GitInspectionDeadlineExceeded
	GitInspectionFilterConfigReadFailed = protocol.GitInspectionFilterConfigReadFailed
	GitInspectionHeadChanged            = protocol.GitInspectionHeadChanged
	GitInspectionHeadReadFailed         = protocol.GitInspectionHeadReadFailed
	GitInspectionHeadRecheckFailed      = protocol.GitInspectionHeadRecheckFailed
	GitInspectionIncompleteHeaders      = protocol.GitInspectionIncompleteHeaders
	GitInspectionIncompleteOutput       = protocol.GitInspectionIncompleteOutput
	GitInspectionIndexChanged           = protocol.GitInspectionIndexChanged
	GitInspectionIndexReadFailed        = protocol.GitInspectionIndexReadFailed
	GitInspectionIndexRecheckFailed     = protocol.GitInspectionIndexRecheckFailed
	GitInspectionInvalidRecord          = protocol.GitInspectionInvalidRecord
	GitInspectionStatusCommandFailed    = protocol.GitInspectionStatusCommandFailed
	GitInspectionStatusNonzeroExit      = protocol.GitInspectionStatusNonzeroExit
	GitInspectionUnsupportedEncoding    = protocol.GitInspectionUnsupportedEncoding
	GitInspectionUnsupportedStatus      = protocol.GitInspectionUnsupportedStatus
	GitInspectionWorktreeOpenFailed     = protocol.GitInspectionWorktreeOpenFailed
	GitMergeKindFastForward             = protocol.GitMergeKindFastForward
	GitMergeKindMergeCommit             = protocol.GitMergeKindMergeCommit
	GitMergeKindUpToDate                = protocol.GitMergeKindUpToDate
	RepositoryKindUnborn                = protocol.RepositoryKindUnborn
	MAX_EDIT_BYTES                      = protocol.MAX_EDIT_BYTES
)

func DefaultGitSync() GitSync { return protocol.DefaultGitSync() }
