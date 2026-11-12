package node

import (
	"context"
	"database/sql"
	"errors"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/claudecli"
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
		catalog, err := node.discoverPackageModels(ctx, binding)
		if err != nil {
			// A model failure keeps its own message: a missing executable or a
			// signed-out program is what the channel editor reports.
			if failure := new(model.Failure); errors.As(err, &failure) {
				return nil, model.Fault(err)
			}
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

// discoverPackageModels lists the models of a channel served by a package:
// those its Claude Code route declares, those its agent program offers, or
// those its translation script discovers.
func (node *Node) discoverPackageModels(ctx context.Context, binding model.Binding) (protocol.ConversationDiscoveryCatalog, error) {
	captured, bound, err := node.captureProgramModel(ctx, binding)
	switch {
	case err != nil:
		return protocol.ConversationDiscoveryCatalog{}, err
	case bound && captured.adapter.ClaudeCode != nil:
		// Claude Code's models are the package's; no process starts.
		capture, err := node.claudeCapture(ctx, captured)
		if err != nil {
			return protocol.ConversationDiscoveryCatalog{}, err
		}
		return claudecli.Discover(binding.Provider.Endpoint, capture)
	case bound:
		// Discovery captures no model selection; the agent's session lists them.
		binding.Session = protocol.NewSessionID()
		return model.DiscoverACP(ctx, binding, node.agentCapture(captured))
	}
	capture, err := node.capturePluginModel(ctx, binding)
	if err != nil {
		return protocol.ConversationDiscoveryCatalog{}, err
	}
	return model.DiscoverPlugin(ctx, binding, capture)
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
