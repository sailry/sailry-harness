package node

import (
	"context"
	"database/sql"
	"reflect"

	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (node *Node) readPluginConnection(ctx context.Context, caller protocol.NodeID, request protocol.Request) (protocol.Output, error) {
	input, valid := dispatchPayload[protocol.CommandReadPluginConnectionPayload](request.Command.Data)
	if !valid {
		return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "plugin connection request is invalid"}
	}
	var value protocol.PluginConnectionStatus
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		scope := request.Plugin
		if scope == nil {
			var err error
			value, err = store.ReadPluginConnection(ctx, db, node.profile.Ownership.Path, input.Package, input.Card)
			return err
		}
		if scope.Surface.Kind == "card" {
			if err := store.CheckPluginCardRequest(ctx, db, node.ID(), caller, request); err != nil {
				return err
			}
			var err error
			value, err = store.ReadPluginConnection(ctx, db, node.profile.Ownership.Path, input.Package, input.Card)
			return err
		}
		var captured *protocol.PluginInfo
		if scope.Invocation != nil {
			node.plugins.mu.Lock()
			invocation := node.plugins.invocations[*scope.Invocation]
			if invocation != nil && caller == node.ID() && reflect.DeepEqual(invocation.scope, *scope) {
				info := invocation.info
				info.Summary.Enabled = true
				captured = &info
			}
			node.plugins.mu.Unlock()
		}
		info, err := store.PluginRequestPackage(ctx, db, node.ID(), caller, request, captured)
		if err != nil {
			return err
		}
		if err := store.CheckPluginConnectionRequest(request, info); err != nil {
			return err
		}
		if err := store.CheckPluginRequest(ctx, db, request, info); err != nil {
			return err
		}
		value, err = store.ReadPluginConnectionAdmitted(ctx, db, node.profile.Ownership.Path, info, input.Card)
		return err
	})
	return protocol.Output{Kind: "plugin_connection", Data: value}, err
}
