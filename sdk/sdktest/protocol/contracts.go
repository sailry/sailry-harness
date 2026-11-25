package protocol

import "github.com/sailry/sailry-harness/internal/protocol"

type (
	ApprovalID                                 = protocol.ApprovalID
	AttachmentID                               = protocol.AttachmentID
	CodeIndexGraph                             = protocol.CodeIndexGraph
	CodeIndexGraphAffected                     = protocol.CodeIndexGraphAffected
	CodeIndexGraphGraphPage                    = protocol.CodeIndexGraphGraphPage
	CodeIndexGraphView                         = protocol.CodeIndexGraphView
	CodeIndexOutline                           = protocol.CodeIndexOutline
	CodeIndexSearch                            = protocol.CodeIndexSearch
	CodeIndexSearchPage                        = protocol.CodeIndexSearchPage
	CodeIndexSettings                          = protocol.CodeIndexSettings
	CodeIndexSymbol                            = protocol.CodeIndexSymbol
	Command                                    = protocol.Command
	CommandBrowseDatabasePayload               = protocol.CommandBrowseDatabasePayload
	CommandCancelCodeIndexQueryPayload         = protocol.CommandCancelCodeIndexQueryPayload
	CommandCheckSshPayload                     = protocol.CommandCheckSshPayload
	CommandCreateSessionPayload                = protocol.CommandCreateSessionPayload
	CommandCreateWorktreePayload               = protocol.CommandCreateWorktreePayload
	CommandDeleteMemoryEntryPayload            = protocol.CommandDeleteMemoryEntryPayload
	CommandGraphCodePayload                    = protocol.CommandGraphCodePayload
	CommandOutlineCodePayload                  = protocol.CommandOutlineCodePayload
	CommandQueryDatabasePayload                = protocol.CommandQueryDatabasePayload
	CommandQueueTurnPayload                    = protocol.CommandQueueTurnPayload
	CommandReadMemoryCatalogPayload            = protocol.CommandReadMemoryCatalogPayload
	CommandReadPluginPayload                   = protocol.CommandReadPluginPayload
	CommandReadPluginValuePayload              = protocol.CommandReadPluginValuePayload
	CommandReadSkillDocumentPayload            = protocol.CommandReadSkillDocumentPayload
	CommandRemoveProjectPayload                = protocol.CommandRemoveProjectPayload
	CommandRemoveWorktreePayload               = protocol.CommandRemoveWorktreePayload
	CommandRenameSessionPayload                = protocol.CommandRenameSessionPayload
	CommandResolveApprovalPayload              = protocol.CommandResolveApprovalPayload
	CommandRunSshPayload                       = protocol.CommandRunSshPayload
	CommandSaveCodeIndexSettingsPayload        = protocol.CommandSaveCodeIndexSettingsPayload
	CommandSaveDatabasePayload                 = protocol.CommandSaveDatabasePayload
	CommandSaveMemoryEntryPayload              = protocol.CommandSaveMemoryEntryPayload
	CommandSaveProviderPayload                 = protocol.CommandSaveProviderPayload
	CommandSaveSshPayload                      = protocol.CommandSaveSshPayload
	CommandSearchCodePayload                   = protocol.CommandSearchCodePayload
	CommandSetPluginEnabledPayload             = protocol.CommandSetPluginEnabledPayload
	CommandSubmitTurnPayload                   = protocol.CommandSubmitTurnPayload
	CommandTransferSshPayload                  = protocol.CommandTransferSshPayload
	CommandTrustSshPayload                     = protocol.CommandTrustSshPayload
	CommandWriteFilePayload                    = protocol.CommandWriteFilePayload
	ComputerSettings                           = protocol.ComputerSettings
	ConnectionResource                         = protocol.ConnectionResource
	ConnectionSharing                          = protocol.ConnectionSharing
	ConversationApproval                       = protocol.ConversationApproval
	ConversationDecision                       = protocol.ConversationDecision
	ConversationHistory                        = protocol.ConversationHistory
	ConversationInput                          = protocol.ConversationInput
	ConversationModel                          = protocol.ConversationModel
	ConversationProvider                       = protocol.ConversationProvider
	ConversationReference                      = protocol.ConversationReference
	ConversationReferenceTarget                = protocol.ConversationReferenceTarget
	ConversationReferenceTargetDatabasePayload = protocol.ConversationReferenceTargetDatabasePayload
	ConversationReferenceTargetSkillPayload    = protocol.ConversationReferenceTargetSkillPayload
	ConversationRun                            = protocol.ConversationRun
	ConversationStatus                         = protocol.ConversationStatus
	DatabaseConnection                         = protocol.DatabaseConnection
	DatabaseConnectionNetworkPayload           = protocol.DatabaseConnectionNetworkPayload
	DatabaseConnectionSqlitePayload            = protocol.DatabaseConnectionSqlitePayload
	DatabaseID                                 = protocol.DatabaseID
	DatabaseProfile                            = protocol.DatabaseProfile
	DatabaseTable                              = protocol.DatabaseTable
	ErrorCode                                  = protocol.ErrorCode
	ExternalBrowserSettings                    = protocol.ExternalBrowserSettings
	Fault                                      = protocol.Fault
	MemoryCatalog                              = protocol.MemoryCatalog
	MemoryEntry                                = protocol.MemoryEntry
	MemorySummary                              = protocol.MemorySummary
	NativeSkill                                = protocol.NativeSkill
	NodeID                                     = protocol.NodeID
	Output                                     = protocol.Output
	OutputCodeIndexQueryCancelledPayload       = protocol.OutputCodeIndexQueryCancelledPayload
	Permission                                 = protocol.Permission
	PluginContext                              = protocol.PluginContext
	PluginConversationBinding                  = protocol.PluginConversationBinding
	PluginDesktopSurface                       = protocol.PluginDesktopSurface
	PluginInfo                                 = protocol.PluginInfo
	PluginReference                            = protocol.PluginReference
	PluginSkillsDocument                       = protocol.PluginSkillsDocument
	PluginSkillsDocumentSource                 = protocol.PluginSkillsDocumentSource
	PluginSkillsDocumentSourceBundledPayload   = protocol.PluginSkillsDocumentSourceBundledPayload
	PluginSummary                              = protocol.PluginSummary
	Project                                    = protocol.Project
	ProjectID                                  = protocol.ProjectID
	ProjectsAppearance                         = protocol.ProjectsAppearance
	ProjectsDraft                              = protocol.ProjectsDraft
	ProjectsSource                             = protocol.ProjectsSource
	ProviderID                                 = protocol.ProviderID
	QueuedTurn                                 = protocol.QueuedTurn
	Request                                    = protocol.Request
	RequestID                                  = protocol.RequestID
	Result[Success, Failure any]               = protocol.Result[Success, Failure]
	RoleID                                     = protocol.RoleID
	RoleReference                              = protocol.RoleReference
	RoleSnapshot                               = protocol.RoleSnapshot
	Secret                                     = protocol.Secret
	Session                                    = protocol.Session
	SessionConfig                              = protocol.SessionConfig
	SessionID                                  = protocol.SessionID
	Snapshot                                   = protocol.Snapshot
	SshCredential                              = protocol.SshCredential
	SshCredentialPasswordPayload               = protocol.SshCredentialPasswordPayload
	SshHostKey                                 = protocol.SshHostKey
	SshID                                      = protocol.SshID
	SshOutcome                                 = protocol.SshOutcome
	SshOutcomeCompletedPayload                 = protocol.SshOutcomeCompletedPayload
	SshProfile                                 = protocol.SshProfile
	SshTransfer                                = protocol.SshTransfer
	SshTrust                                   = protocol.SshTrust
	ToolOperation                              = protocol.ToolOperation
	Topic                                      = protocol.Topic
	TurnID                                     = protocol.TurnID
	Worktree                                   = protocol.Worktree
	WorktreeID                                 = protocol.WorktreeID
)

const (
	AuthenticationApiKey                 = protocol.AuthenticationApiKey
	CodeIndexGraphViewCallees            = protocol.CodeIndexGraphViewCallees
	CodeIndexGraphViewCallers            = protocol.CodeIndexGraphViewCallers
	CodeIndexGraphViewImpact             = protocol.CodeIndexGraphViewImpact
	CodeIndexGraphViewMap                = protocol.CodeIndexGraphViewMap
	ConversationApprovalSourceUser       = protocol.ConversationApprovalSourceUser
	ConversationApprovalStateApproved    = protocol.ConversationApprovalStateApproved
	ConversationApprovalStatePending     = protocol.ConversationApprovalStatePending
	ConversationExecutionModel           = protocol.ConversationExecutionModel
	ConversationModelApiChatCompletions  = protocol.ConversationModelApiChatCompletions
	ConversationStatusCancelled          = protocol.ConversationStatusCancelled
	ConversationStatusCompleted          = protocol.ConversationStatusCompleted
	ConversationStatusFailed             = protocol.ConversationStatusFailed
	ConversationStatusInterrupted        = protocol.ConversationStatusInterrupted
	ConversationStatusQueued             = protocol.ConversationStatusQueued
	ConversationStatusRunning            = protocol.ConversationStatusRunning
	DatabaseEnginePostgres               = protocol.DatabaseEnginePostgres
	DatabaseTlsRequire                   = protocol.DatabaseTlsRequire
	ErrorCodeCancelled                   = protocol.ErrorCodeCancelled
	ErrorCodeInvalidRequest              = protocol.ErrorCodeInvalidRequest
	ErrorCodeNotConfigured               = protocol.ErrorCodeNotConfigured
	ErrorCodeNotFound                    = protocol.ErrorCodeNotFound
	ErrorCodePermissionDenied            = protocol.ErrorCodePermissionDenied
	ErrorCodeRevisionConflict            = protocol.ErrorCodeRevisionConflict
	ErrorCodeUnavailable                 = protocol.ErrorCodeUnavailable
	ErrorCodeWrongTarget                 = protocol.ErrorCodeWrongTarget
	PermissionAsk                        = protocol.PermissionAsk
	PermissionFull                       = protocol.PermissionFull
	PermissionProject                    = protocol.PermissionProject
	PluginStorageScopeNode               = protocol.PluginStorageScopeNode
	SshAuthenticationAgent               = protocol.SshAuthenticationAgent
	SshAuthenticationPassword            = protocol.SshAuthenticationPassword
	SshDirectionUpload                   = protocol.SshDirectionUpload
	ToolOperationBrowseDatabase          = protocol.ToolOperationBrowseDatabase
	ToolOperationExecuteDatabase         = protocol.ToolOperationExecuteDatabase
	ToolOperationListDatabaseConnections = protocol.ToolOperationListDatabaseConnections
	ToolOperationListSshConnections      = protocol.ToolOperationListSshConnections
	ToolOperationQueryDatabase           = protocol.ToolOperationQueryDatabase
	ToolOperationRunSsh                  = protocol.ToolOperationRunSsh
	ToolOperationSaveDatabaseConnection  = protocol.ToolOperationSaveDatabaseConnection
	ToolOperationSaveSshConnection       = protocol.ToolOperationSaveSshConnection
	ToolOperationTransferSsh             = protocol.ToolOperationTransferSsh
	ToolOperationWriteFile               = protocol.ToolOperationWriteFile
	WorkModeCode                         = protocol.WorkModeCode
	WorkModeFlagship                     = protocol.WorkModeFlagship
)

func DefaultConversationApproval() ConversationApproval {
	return protocol.DefaultConversationApproval()
}

func DefaultConversationModel() ConversationModel { return protocol.DefaultConversationModel() }

func DefaultConversationProvider() ConversationProvider {
	return protocol.DefaultConversationProvider()
}

func DefaultPluginContext() PluginContext { return protocol.DefaultPluginContext() }

func DefaultProjectsAppearance() ProjectsAppearance { return protocol.DefaultProjectsAppearance() }

func DefaultRoleSnapshot() RoleSnapshot { return protocol.DefaultRoleSnapshot() }

func DefaultSessionConfig() SessionConfig { return protocol.DefaultSessionConfig() }

func NewApprovalID() ApprovalID { return protocol.NewApprovalID() }

func NewAttachmentID() AttachmentID { return protocol.NewAttachmentID() }

func NewDatabaseID() DatabaseID { return protocol.NewDatabaseID() }

func NewProviderID() ProviderID { return protocol.NewProviderID() }

func NewRequest(target NodeID, command Command) Request { return protocol.NewRequest(target, command) }

func NewRoleID() RoleID { return protocol.NewRoleID() }

func NewSecret(value string) Secret { return protocol.NewSecret(value) }

func NewSessionID() SessionID { return protocol.NewSessionID() }

func NewSshID() SshID { return protocol.NewSshID() }

func ParseRequestID(value string) (RequestID, error) { return protocol.ParseRequestID(value) }
