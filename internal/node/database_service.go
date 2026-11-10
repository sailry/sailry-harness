package node

import (
	"context"
	"database/sql"
	"reflect"
	"sync"

	"github.com/sailry/sailry-harness/internal/databases"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func databaseCommand(kind string) bool {
	switch kind {
	case "test_database", "check_database", "browse_database", "query_database", "cancel_database":
		return true
	}
	return false
}

type databaseOwner struct {
	caller  protocol.NodeID
	request protocol.RequestID
}

type databaseActive struct {
	cancel context.CancelFunc
	plugin *protocol.PluginContext
}

// databaseOperations owns the cancellation of every running external database
// operation by its original caller and request.
type databaseOperations struct {
	mu     sync.Mutex
	active map[databaseOwner]databaseActive
}

// dispatchDatabase executes one external database operation outside any store
// writer. A cancellation from another plugin scope is refused; cancelling a
// settled request succeeds without effect.
func (node *Node) dispatchDatabase(ctx context.Context, caller protocol.NodeID, request protocol.Request) (bool, protocol.Output, error) {
	if !databaseCommand(request.Command.Kind) {
		return false, protocol.Output{}, nil
	}
	operations := &node.databases
	if input, ok := request.Command.Data.(*protocol.CommandCancelDatabasePayload); ok {
		operations.mu.Lock()
		active, running := operations.active[databaseOwner{caller, input.Request}]
		operations.mu.Unlock()
		if running {
			if request.Plugin != nil && !reflect.DeepEqual(request.Plugin, active.plugin) {
				return true, protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "connection request belongs to another plugin scope"}
			}
			active.cancel()
		}
		return true, protocol.Output{Kind: "database_cancelled", Data: protocol.OutputDatabaseCancelledPayload{Request: input.Request}}, nil
	}
	var prepared store.DatabaseOperation
	if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		prepared, err = store.PrepareDatabase(ctx, db, node.profile.Ownership.Path, request.Command)
		return err
	}); err != nil {
		return true, protocol.Output{}, err
	}
	operation := databases.Operation{Profile: prepared.Profile, Password: prepared.Password, Storage: prepared.Storage}
	if prepared.Ssh != nil {
		operation.SSH = &databases.SSH{Profile: prepared.Ssh.Profile, Credential: prepared.Ssh.Credential}
	}
	if prepared.Query != nil {
		operation.Query = &databases.Query{SQL: prepared.Query.Sql, RowLimit: uint32(prepared.Query.RowLimit), TimeoutMS: prepared.Query.TimeoutMs}
	}
	if prepared.Browse != nil {
		operation.Browse = &databases.Browse{Database: prepared.Browse.Database}
	}
	scoped, cancel := context.WithCancel(ctx)
	owner := databaseOwner{caller, request.ID}
	operations.mu.Lock()
	if operations.active == nil {
		operations.active = map[databaseOwner]databaseActive{}
	}
	operations.active[owner] = databaseActive{cancel: cancel, plugin: request.Plugin}
	operations.mu.Unlock()
	defer func() {
		operations.mu.Lock()
		delete(operations.active, owner)
		operations.mu.Unlock()
		cancel()
	}()
	outcome, err := databases.Execute(scoped, operation)
	return true, protocol.Output{Kind: "database_outcome", Data: outcome}, err
}
