package node

import (
	"context"
	"database/sql"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/providers"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (node *Node) prepareProvider(ctx context.Context, caller protocol.NodeID, request protocol.Request) (*providers.Prepared, error) {
	if request.Plugin != nil {
		return nil, &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "plugin invocation authority is unavailable"}
	}
	var work *providers.Work
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		work, err = store.PrepareProviderCommand(ctx, db, node.ID(), caller, request)
		return err
	})
	if err != nil || work == nil {
		return nil, err
	}
	if work.Provider != nil && model.PluginBound(*work.Provider) {
		binding := model.Binding{Provider: *work.Provider, Secret: work.Secret}
		capture, err := node.capturePluginModel(ctx, binding)
		if err != nil {
			return nil, err
		}
		catalog, err := model.DiscoverPlugin(ctx, binding, capture)
		if err != nil {
			return nil, err
		}
		output := protocol.Output{Kind: "discovered_models", Data: catalog}
		if request.Command.Kind == "validate_provider" {
			output = protocol.Output{Kind: "provider_validation", Data: providers.ValidateDiscovery(*work.Provider, catalog.Models)}
		}
		return &providers.Prepared{Provider: work.Provider, Output: &output}, nil
	}
	work.Directory = &node.providerDirectory
	return providers.Run(ctx, *work)
}
func (node *Node) readProvider(ctx context.Context, caller protocol.NodeID, request protocol.Request) link.Response {
	prepared, err := node.prepareProvider(ctx, caller, request)
	if err != nil {
		return response(protocol.Output{}, err)
	}
	if prepared != nil {
		unlock := node.agent.LockPublication()
		defer unlock()
		var output protocol.Output
		var changes []protocol.Event
		err = node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
			var err error
			output, changes, err = store.ExecuteProviderCommand(ctx, db, node.ID(), caller, request, prepared)
			if err != nil {
				return err
			}
			_, err = store.RecordEvents(ctx, db, node.ID(), changes)
			return err
		})
		if err == nil {
			node.publish()
			node.conversationEvents(changes)
		}
		return response(output, err)
	}
	var output protocol.Output
	err = node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		output, err = store.ReadProviderCommand(ctx, db, node.ID(), request.Command)
		return err
	})
	return response(output, err)
}
