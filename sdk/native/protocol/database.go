package protocol

import "github.com/sailry/sailry-harness/internal/protocol"

type (
	CommandBrowseDatabasePayload = protocol.CommandBrowseDatabasePayload
	CommandCancelDatabasePayload = protocol.CommandCancelDatabasePayload
	CommandSaveDatabasePayload   = protocol.CommandSaveDatabasePayload
	CommandTestDatabasePayload   = protocol.CommandTestDatabasePayload
	DatabaseID                   = protocol.DatabaseID
	HostPath                     = protocol.HostPath
)

func NewDatabaseID() DatabaseID { return protocol.NewDatabaseID() }
