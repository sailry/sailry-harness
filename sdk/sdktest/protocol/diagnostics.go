package protocol

import "github.com/sailry/sailry-harness/internal/protocol"

type (
	CommandApplyGitHunkPayload         = protocol.CommandApplyGitHunkPayload
	CommandCancelSshPayload            = protocol.CommandCancelSshPayload
	CommandInspectGitPayload           = protocol.CommandInspectGitPayload
	CommandInstallBundledPluginPayload = protocol.CommandInstallBundledPluginPayload
	CommandInstallHostPayload          = protocol.CommandInstallHostPayload
	CommandListDirectoryPayload        = protocol.CommandListDirectoryPayload
	CommandListWorktreesPayload        = protocol.CommandListWorktreesPayload
	CommandReadFilePayload             = protocol.CommandReadFilePayload
	CommandReadGitDiffPayload          = protocol.CommandReadGitDiffPayload
	CommandReadPluginCallbackPayload   = protocol.CommandReadPluginCallbackPayload
	CommandRegisterProjectPayload      = protocol.CommandRegisterProjectPayload
	ConversationDiscoveryCapabilities  = protocol.ConversationDiscoveryCapabilities
	FileContent                        = protocol.FileContent
	GitDiff                            = protocol.GitDiff
	NotificationSystemSystemDiagnostic = protocol.NotificationSystemSystemDiagnostic
	OutputSshCancelledPayload          = protocol.OutputSshCancelledPayload
	SSHFailureReason                   = protocol.SSHFailureReason
	SshOutcomeHostKeyRequiredPayload   = protocol.SshOutcomeHostKeyRequiredPayload
)

const (
	ErrorCodeOutcomeUnknown                      = protocol.ErrorCodeOutcomeUnknown
	GitDiffScopeUnstaged                         = protocol.GitDiffScopeUnstaged
	GitHunkStage                                 = protocol.GitHunkStage
	GitInspectionStatusCommandFailed             = protocol.GitInspectionStatusCommandFailed
	NotificationSystemDiagnosticPhaseCallback    = protocol.NotificationSystemDiagnosticPhaseCallback
	NotificationSystemDiagnosticReasonFailure    = protocol.NotificationSystemDiagnosticReasonFailure
	NotificationSystemSystemSourcePluginCallback = protocol.NotificationSystemSystemSourcePluginCallback
	SSHAuthFailed                                = protocol.SSHAuthFailed
	SSHCommandInterrupted                        = protocol.SSHCommandInterrupted
	SSHCommandNonzeroExit                        = protocol.SSHCommandNonzeroExit
)
