package node

import (
	"context"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/providers"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (node *Node) newModel(ctx context.Context, binding model.Binding) (fantasy.LanguageModel, error) {
	if node.modelFactory != nil {
		return node.modelFactory(ctx, binding)
	}
	if model.PluginBound(binding.Provider) {
		capture, err := node.capturePluginModel(ctx, binding)
		if err != nil {
			return nil, err
		}
		return model.NewPlugin(ctx, binding, capture)
	}
	return model.New(ctx, binding)
}

func (node *Node) capturePluginModel(ctx context.Context, binding model.Binding) (model.PluginCapture, error) {
	var captured model.PluginCapture
	if !model.PluginBound(binding.Provider) || (**binding.Provider.Binding).Package == nil {
		return captured, &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "captured model adapter is unavailable"}
	}
	selection := **binding.Provider.Binding
	info, err := node.plugins.manager.CapturedInfo(ctx, *selection.Package)
	if err != nil {
		return captured, err
	}
	if info.Extension == nil {
		return captured, &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "captured model adapter is unavailable"}
	}
	found := false
	for _, declaration := range info.Extension.ModelAdapters {
		if declaration.ID == selection.Adapter.Adapter && declaration.Capabilities == selection.Adapter.Capabilities {
			captured.Declaration, found = declaration, true
			break
		}
	}
	if !found {
		return captured, &protocol.Fault{Code: protocol.ErrorCodeRevisionConflict, Message: "captured model adapter differs from its declaration"}
	}
	files, err := node.plugins.manager.ReadBundle(ctx, *selection.Package, captured.Declaration.Script.Entry, captured.Declaration.Script.Resources)
	if err != nil {
		return captured, err
	}
	captured.Bundle = plugins.ScriptBundle{Entry: captured.Declaration.Script.Entry, Files: files}
	if binding.Provider.Authentication == protocol.AuthenticationApiKey {
		captured.Key = binding.Secret
		return captured, nil
	}
	grant, headers, err := providers.AccountHeaders(binding.Provider, binding.Secret)
	if err != nil {
		return captured, err
	}
	captured.Headers, captured.Secrets = headers, grant.Secrets()
	access := grant.Access
	if grant.Record != nil {
		access = grant.Record.Issued.Access
	}
	if access != "" {
		value := protocol.NewSecret(access)
		captured.Access = &value
	}
	return captured, nil
}
