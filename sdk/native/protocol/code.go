package protocol

import "github.com/sailry/sailry-harness/internal/protocol"

type (
	CodeIndexCapabilities               = protocol.CodeIndexCapabilities
	CodeIndexGraph                      = protocol.CodeIndexGraph
	CodeIndexGraphAffected              = protocol.CodeIndexGraphAffected
	CodeIndexGraphConfidence            = protocol.CodeIndexGraphConfidence
	CodeIndexGraphDefinition            = protocol.CodeIndexGraphDefinition
	CodeIndexGraphEntry                 = protocol.CodeIndexGraphEntry
	CodeIndexGraphGraphPage             = protocol.CodeIndexGraphGraphPage
	CodeIndexGraphImpacted              = protocol.CodeIndexGraphImpacted
	CodeIndexGraphRanked                = protocol.CodeIndexGraphRanked
	CodeIndexGraphReference             = protocol.CodeIndexGraphReference
	CodeIndexGraphView                  = protocol.CodeIndexGraphView
	CodeIndexIssue                      = protocol.CodeIndexIssue
	CodeIndexOutline                    = protocol.CodeIndexOutline
	CodeIndexRange                      = protocol.CodeIndexRange
	CodeIndexSearch                     = protocol.CodeIndexSearch
	CodeIndexSearchPage                 = protocol.CodeIndexSearchPage
	CodeIndexSettings                   = protocol.CodeIndexSettings
	CodeIndexStatus                     = protocol.CodeIndexStatus
	CodeIndexSymbol                     = protocol.CodeIndexSymbol
	Command                             = protocol.Command
	CommandCancelCodeIndexQueryPayload  = protocol.CommandCancelCodeIndexQueryPayload
	CommandGraphCodePayload             = protocol.CommandGraphCodePayload
	CommandOutlineCodePayload           = protocol.CommandOutlineCodePayload
	CommandSaveCodeIndexSettingsPayload = protocol.CommandSaveCodeIndexSettingsPayload
	CommandSearchCodePayload            = protocol.CommandSearchCodePayload
	DatabaseEngine                      = protocol.DatabaseEngine
	DatabaseProfile                     = protocol.DatabaseProfile
	ErrorCode                           = protocol.ErrorCode
	Fault                               = protocol.Fault
	FileLines                           = protocol.FileLines
	NodeID                              = protocol.NodeID
	Output                              = protocol.Output
	Project                             = protocol.Project
	ProjectsAppearance                  = protocol.ProjectsAppearance
	ProjectsDraft                       = protocol.ProjectsDraft
	ProjectsSource                      = protocol.ProjectsSource
	Request                             = protocol.Request
	RequestID                           = protocol.RequestID
	SshID                               = protocol.SshID
	ToolOperation                       = protocol.ToolOperation
	Update                              = protocol.Update
	WorktreeID                          = protocol.WorktreeID
)

const (
	CodeIndexGraphConfidenceDirectory = protocol.CodeIndexGraphConfidenceDirectory
	CodeIndexGraphConfidenceImported  = protocol.CodeIndexGraphConfidenceImported
	CodeIndexGraphConfidenceLocal     = protocol.CodeIndexGraphConfidenceLocal
	CodeIndexGraphConfidenceWorkspace = protocol.CodeIndexGraphConfidenceWorkspace
	CodeIndexGraphViewCallees         = protocol.CodeIndexGraphViewCallees
	CodeIndexGraphViewCallers         = protocol.CodeIndexGraphViewCallers
	CodeIndexGraphViewContext         = protocol.CodeIndexGraphViewContext
	CodeIndexGraphViewImpact          = protocol.CodeIndexGraphViewImpact
	CodeIndexGraphViewMap             = protocol.CodeIndexGraphViewMap
	CodeIndexStatusFailed             = protocol.CodeIndexStatusFailed
	CodeIndexStatusInvalid            = protocol.CodeIndexStatusInvalid
	CodeIndexStatusParsed             = protocol.CodeIndexStatusParsed
	CodeIndexStatusTooLarge           = protocol.CodeIndexStatusTooLarge
	CodeIndexStatusUnsupported        = protocol.CodeIndexStatusUnsupported
	DatabaseEngineMysql               = protocol.DatabaseEngineMysql
	DatabaseEnginePostgres            = protocol.DatabaseEnginePostgres
	DatabaseEngineSqlite              = protocol.DatabaseEngineSqlite
	ErrorCodeBusy                     = protocol.ErrorCodeBusy
	ErrorCodeCancelled                = protocol.ErrorCodeCancelled
	ErrorCodeInternal                 = protocol.ErrorCodeInternal
	ErrorCodeInvalidRequest           = protocol.ErrorCodeInvalidRequest
	ErrorCodeNotConfigured            = protocol.ErrorCodeNotConfigured
	ErrorCodeNotFound                 = protocol.ErrorCodeNotFound
	ErrorCodePermissionDenied         = protocol.ErrorCodePermissionDenied
	ErrorCodeRevisionConflict         = protocol.ErrorCodeRevisionConflict
	ErrorCodeUnavailable              = protocol.ErrorCodeUnavailable
	ToolOperationFindFiles            = protocol.ToolOperationFindFiles
	ToolOperationGraphCode            = protocol.ToolOperationGraphCode
	ToolOperationOutlineCode          = protocol.ToolOperationOutlineCode
	ToolOperationReadFileLines        = protocol.ToolOperationReadFileLines
	ToolOperationSearchCode           = protocol.ToolOperationSearchCode
	ToolOperationSearchFiles          = protocol.ToolOperationSearchFiles
)

func DefaultCodeIndexGraph() CodeIndexGraph { return protocol.DefaultCodeIndexGraph() }

func DefaultCodeIndexGraphGraphPage() CodeIndexGraphGraphPage {
	return protocol.DefaultCodeIndexGraphGraphPage()
}

func DefaultCodeIndexOutline() CodeIndexOutline { return protocol.DefaultCodeIndexOutline() }

func DefaultCodeIndexSearch() CodeIndexSearch { return protocol.DefaultCodeIndexSearch() }

func DefaultCodeIndexSearchPage() CodeIndexSearchPage { return protocol.DefaultCodeIndexSearchPage() }

func DefaultProjectsAppearance() ProjectsAppearance { return protocol.DefaultProjectsAppearance() }

func NewRequest(target NodeID, command Command) Request { return protocol.NewRequest(target, command) }

func NewRequestID() RequestID { return protocol.NewRequestID() }

func ParseWorktreeID(value string) (WorktreeID, error) { return protocol.ParseWorktreeID(value) }
