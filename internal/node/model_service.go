package node

import (
	"context"
	"database/sql"
	"path/filepath"
	"slices"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/providers"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (node *Node) newModel(ctx context.Context, binding model.Binding) (fantasy.LanguageModel, error) {
	if node.modelFactory != nil {
		return node.modelFactory(ctx, binding)
	}
	if model.PluginBound(binding.Provider) {
		if agent, bound, err := node.captureAgentModel(ctx, binding); err != nil {
			return nil, err
		} else if bound {
			return model.NewACP(binding, agent)
		}
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
	captured.Memory = node.plugins.memory
	// A setting is the user's current choice, so the installed package's
	// values are read now, not those of the provider's captured revision.
	captured.Settings = plugins.SettingDefaults(info.Settings)
	err = node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		current, err := store.ReadPlugin(ctx, db, selection.Package.Name)
		if err != nil || current == nil || current.Settings == nil {
			return err
		}
		resolved, err := store.ResolvePluginSettings(ctx, db, plugins.PackageReference(*current))
		if err == nil {
			captured.Settings = resolved.Values
		}
		return err
	})
	if err != nil {
		return captured, err
	}
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

// captureAgentModel captures an adapter whose transport is a declared
// program. bound is false for an adapter that translates HTTP.
func (node *Node) captureAgentModel(ctx context.Context, binding model.Binding) (captured model.ACPCapture, bound bool, err error) {
	unavailable := &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "captured model adapter is unavailable"}
	if !model.PluginBound(binding.Provider) || (**binding.Provider.Binding).Package == nil {
		return captured, false, unavailable
	}
	selection := **binding.Provider.Binding
	info, err := node.plugins.manager.CapturedInfo(ctx, *selection.Package)
	if err != nil {
		return captured, false, err
	}
	if info.Extension == nil {
		return captured, false, unavailable
	}
	for _, declaration := range info.Extension.ModelAdapters {
		if declaration.ID != selection.Adapter.Adapter || declaration.Acp == nil {
			continue
		}
		if declaration.Capabilities != selection.Adapter.Capabilities {
			return captured, false, &protocol.Fault{Code: protocol.ErrorCodeRevisionConflict, Message: "captured model adapter differs from its declaration"}
		}
		// Starting a program is its own action; the user saw it when the package was installed or updated.
		if !slices.Contains(info.Extension.Actions, protocol.PluginActionRunPrograms) {
			return captured, false, &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "plugin may not run programs"}
		}
		for _, declared := range info.Extension.Programs {
			if declared.ID == declaration.Acp.Program {
				return model.ACPCapture{
					Adapter: declaration, Program: declared, Owner: info.Summary.Name, Version: link.ProductVersion,
					Dir:  filepath.Join(node.profile.Ownership.Path, "plugins", "data", info.Summary.Name, "programs", declared.ID),
					Host: node.environment.TerminalValues(), Pool: node.plugins.programs,
				}, true, nil
			}
		}
		return captured, false, unavailable
	}
	return captured, false, nil
}
