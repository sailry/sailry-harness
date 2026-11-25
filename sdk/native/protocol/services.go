package protocol

import "github.com/sailry/sailry-harness/internal/protocol"

type (
	Bytes                            = protocol.Bytes
	CommandBrowseSshDirectoryPayload = protocol.CommandBrowseSshDirectoryPayload
	CommandCancelSshPayload          = protocol.CommandCancelSshPayload
	CommandCheckSshPayload           = protocol.CommandCheckSshPayload
	CommandDownloadSshFilePayload    = protocol.CommandDownloadSshFilePayload
	CommandFinishSshUploadPayload    = protocol.CommandFinishSshUploadPayload
	CommandInstallHostPayload        = protocol.CommandInstallHostPayload
	CommandModifySshFilePayload      = protocol.CommandModifySshFilePayload
	CommandOpenSshTerminalPayload    = protocol.CommandOpenSshTerminalPayload
	CommandRunSshPayload             = protocol.CommandRunSshPayload
	CommandTransferSshPayload        = protocol.CommandTransferSshPayload
	CommandUpdateSshHostPayload      = protocol.CommandUpdateSshHostPayload
	ConnectionResource               = protocol.ConnectionResource
	DatabaseCatalog                  = protocol.DatabaseCatalog
	DatabaseCatalogTablesPayload     = protocol.DatabaseCatalogTablesPayload
	DatabaseConnection               = protocol.DatabaseConnection
	DatabaseConnectionNetworkPayload = protocol.DatabaseConnectionNetworkPayload
	DatabaseConnectionSocketPayload  = protocol.DatabaseConnectionSocketPayload
	DatabaseConnectionSqlitePayload  = protocol.DatabaseConnectionSqlitePayload
	DatabaseConnectionSshPayload     = protocol.DatabaseConnectionSshPayload
	DatabaseOutcome                  = protocol.DatabaseOutcome
	DatabaseResultSet                = protocol.DatabaseResultSet
	DatabaseServer                   = protocol.DatabaseServer
	DatabaseTable                    = protocol.DatabaseTable
	DatabaseTls                      = protocol.DatabaseTls
	DatabaseValue                    = protocol.DatabaseValue
	EntryKind                        = protocol.EntryKind
	FileEntry                        = protocol.FileEntry
	NetworkSelection                 = protocol.NetworkSelection
	OutputSshCancelledPayload        = protocol.OutputSshCancelledPayload
	SSHFailureReason                 = protocol.SSHFailureReason
	Secret                           = protocol.Secret
	SshAuthentication                = protocol.SshAuthentication
	SshCredential                    = protocol.SshCredential
	SshCredentialKeyPathPayload      = protocol.SshCredentialKeyPathPayload
	SshCredentialPasswordPayload     = protocol.SshCredentialPasswordPayload
	SshCredentialPrivateKeyPayload   = protocol.SshCredentialPrivateKeyPayload
	SshCursor                        = protocol.SshCursor
	SshDirection                     = protocol.SshDirection
	SshDirectory                     = protocol.SshDirectory
	SshDownload                      = protocol.SshDownload
	SshFileAction                    = protocol.SshFileAction
	SshFileActionCopyPayload         = protocol.SshFileActionCopyPayload
	SshFileActionCreatePayload       = protocol.SshFileActionCreatePayload
	SshFileActionMovePayload         = protocol.SshFileActionMovePayload
	SshHostKey                       = protocol.SshHostKey
	SshOutcome                       = protocol.SshOutcome
	SshOutcomeCompletedPayload       = protocol.SshOutcomeCompletedPayload
	SshOutcomeHostKeyRequiredPayload = protocol.SshOutcomeHostKeyRequiredPayload
	SshOutcomeTransferredPayload     = protocol.SshOutcomeTransferredPayload
	SshProfile                       = protocol.SshProfile
	SshTransfer                      = protocol.SshTransfer
	SshUpload                        = protocol.SshUpload
	SshUploadSpec                    = protocol.SshUploadSpec
	StreamID                         = protocol.StreamID
	TerminalID                       = protocol.TerminalID
	UserID                           = protocol.UserID
)

const (
	DatabaseTlsDisable        = protocol.DatabaseTlsDisable
	DatabaseTlsPrefer         = protocol.DatabaseTlsPrefer
	DatabaseTlsRequire        = protocol.DatabaseTlsRequire
	EntryKindDirectory        = protocol.EntryKindDirectory
	EntryKindFile             = protocol.EntryKindFile
	EntryKindOther            = protocol.EntryKindOther
	EntryKindSymlink          = protocol.EntryKindSymlink
	ErrorCodeConflict         = protocol.ErrorCodeConflict
	ErrorCodeOutcomeUnknown   = protocol.ErrorCodeOutcomeUnknown
	FILE_TRANSFER_CHUNK_BYTES = protocol.FILE_TRANSFER_CHUNK_BYTES
	FILE_UPLOAD_READY         = protocol.FILE_UPLOAD_READY
	MAX_DIRECTORY_ENTRIES     = protocol.MAX_DIRECTORY_ENTRIES
	SSHAgentUnavailable       = protocol.SSHAgentUnavailable
	SSHAuthCancelled          = protocol.SSHAuthCancelled
	SSHAuthFailed             = protocol.SSHAuthFailed
	SSHAuthTimeout            = protocol.SSHAuthTimeout
	SSHCredentialInvalid      = protocol.SSHCredentialInvalid
	SSHDialCancelled          = protocol.SSHDialCancelled
	SSHDialFailed             = protocol.SSHDialFailed
	SSHDialTimeout            = protocol.SSHDialTimeout
	SSHHandshakeCancelled     = protocol.SSHHandshakeCancelled
	SSHHandshakeFailed        = protocol.SSHHandshakeFailed
	SSHHandshakeTimeout       = protocol.SSHHandshakeTimeout
	SSHKeyFileUnavailable     = protocol.SSHKeyFileUnavailable
	SshAuthenticationPassword = protocol.SshAuthenticationPassword
	SshDirectionDownload      = protocol.SshDirectionDownload
	SshDirectionUpload        = protocol.SshDirectionUpload
)

func NewSecret(value string) Secret { return protocol.NewSecret(value) }

func NewSshID() SshID { return protocol.NewSshID() }
