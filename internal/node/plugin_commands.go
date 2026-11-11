package node

import (
	"context"
	"database/sql"
	"io"
	"slices"
	"time"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/admission"
	"github.com/sailry/sailry-harness/internal/node/authorization"
	"github.com/sailry/sailry-harness/internal/node/installations"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/python"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func pluginCommand(kind string) bool {
	switch kind {
	case "list_plugins", "read_plugin", "read_plugin_version", "read_plugin_settings", "read_plugin_secret", "save_plugin_settings", "set_plugin_enabled", "remove_plugin",
		"install_plugin", "install_plugin_source", "install_bundled_plugin", "install_plugin_upload", "upload_plugin", "inspect_plugin_upload",
		"search_plugin_catalog", "search_skill_catalog", "read_catalog_plugin", "read_catalog_plugin_info", "inspect_plugin_source", "check_plugin_update", "check_plugin_updates",
		"discover_skills", "discover_registry_skill", "install_registry_skill", "install_skill", "read_skill_document", "discover_skill_upload", "discover_local_skills", "install_skill_upload",
		"read_plugin_value", "write_plugin_value", "write_indexed_plugin_value", "remove_plugin_value", "list_plugin_keys", "search_plugin_values",
		"read_plugin_conversation_value", "write_plugin_conversation_value", "remove_plugin_conversation_value", "plugin_transaction",
		"call_plugin", "read_plugin_callback", "cancel_plugin_call", "request_plugin_http", "dispatch", "stop_dispatch_turn",
		"list_plugin_models", "resolve_plugin_model", "generate_plugin_text", "request_provider_http", "execute_provider_http",
		"read_installations", "dismiss_installation", "read_plugin_mcp", "save_plugin_mcp", "set_plugin_mcp_allowed", "check_plugin_mcp", "install_mcp",
		"list_connector_types", "list_connections", "start_connector_login", "submit_connector_input", "cancel_connector_login", "update_connection", "set_connection_enabled", "disconnect_connection", "remove_connection", "confirm_connector_consent", "probe_connection", "inspect_connector_cli",
		"begin_provider_login", "complete_provider_login", "cancel_provider_login", "read_python_settings", "save_python_settings", "read_plugin_view", "read_plugin_connection":
		return true
	default:
		return false
	}
}

func installationResource(command protocol.Command) (protocol.InstallationResource, bool, bool) {
	var name string
	observe := true
	switch value := command.Data.(type) {
	case *protocol.CommandInstallPluginPayload:
		name = value.Name
	case *protocol.CommandInstallBundledPluginPayload:
		name = value.Name
	case *protocol.CommandInstallPluginSourcePayload:
		name = value.Name
	case *protocol.CommandInstallPluginUploadPayload:
		name = value.Name
	case *protocol.CommandInstallSkillPayload:
		name = value.Name
	case *protocol.CommandInstallRegistrySkillPayload:
		name = value.Name
	case *protocol.CommandInstallSkillUploadPayload:
		name = value.Name
	case *protocol.CommandInstallMcpPayload:
		name = value.Name
	case *protocol.CommandRemovePluginPayload:
		name, observe = value.Name, false
	default:
		return protocol.InstallationResource{}, false, false
	}
	return protocol.InstallationResource{Kind: "plugin", Data: protocol.InstallationResourcePluginPayload{Name: name}}, observe, true
}

func (node *Node) dispatchPlugins(ctx context.Context, caller protocol.NodeID, request protocol.Request, body []byte) (link.Admission, error) {
	if err := node.begin(); err != nil {
		return link.Admission{}, err
	}
	completion := make(chan link.Response, 1)
	accepted := link.Admission{Receipt: protocol.Receipt{ID: request.ID, Durable: request.Command.Durable()}, Completion: completion}
	if accepted.Receipt.Durable {
		var previous *link.Response
		err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
			var err error
			previous, err = admission.Admit(ctx, db, caller, request, body)
			return err
		})
		if err != nil {
			node.work.Done()
			return link.Admission{}, nodeFault(err)
		}
		if previous != nil {
			node.work.Done()
			completion <- *previous
			close(completion)
			return accepted, nil
		}
	}
	work := func(owner context.Context, report installations.Report) (result link.Response) {
		defer node.work.Done()
		defer func() {
			if recover() != nil {
				result = response(protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "plugin operation did not settle; inspect the resource before retrying"})
				if accepted.Receipt.Durable {
					recording := context.WithoutCancel(node.ctx)
					_ = node.profile.Store.Node.Write(recording, func(db *sql.Conn) error {
						return admission.Complete(recording, db, caller, request.ID, result)
					})
				}
				node.authCommitted(caller, request, result.Fault)
			}
			completion <- result
			close(completion)
		}()
		operation := owner
		if !accepted.Receipt.Durable {
			operation = ctx
		}
		if accepted.Receipt.Durable {
			result = node.executePlugin(operation, caller, request, report)
		} else {
			output, err := node.readPluginCommand(operation, caller, request)
			result = response(output, err)
		}
		return result
	}
	var err error
	if resource, observe, installation := installationResource(request.Command); installation {
		err = node.plugins.installations.Submit(caller, request, resource, observe, work)
	} else {
		err = node.tasks.Submit(func(owner context.Context) {
			work(owner, func(protocol.InstallationPhase, *protocol.InstallationProgress) {})
		})
	}
	if err != nil {
		result := response(protocol.Output{}, err)
		if accepted.Receipt.Durable {
			recordErr := node.profile.Store.Node.Write(context.WithoutCancel(node.ctx), func(db *sql.Conn) error {
				return admission.Complete(context.WithoutCancel(node.ctx), db, caller, request.ID, result)
			})
			if recordErr != nil {
				result = response(protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "installation admission result could not be recorded"})
			}
		}
		node.work.Done()
		completion <- result
		close(completion)
	}
	return accepted, nil
}

type pluginPreparation struct {
	info       *protocol.PluginInfo
	output     *protocol.Output
	mcp        protocol.PluginMcpConfiguration
	mcpTargets map[string]string
	release    func() error
}

func (node *Node) preparePluginCommand(ctx context.Context, caller protocol.NodeID, request protocol.Request, report installations.Report) (pluginPreparation, error) {
	var prepared pluginPreparation
	if request.Command.Kind == "plugin_transaction" {
		if err := node.renewCommandProvider(ctx, request); err != nil {
			return prepared, err
		}
	}
	if output, handled, err := node.prepareAuthCommand(ctx, caller, request); handled {
		prepared.output = &output
		return prepared, err
	}
	manager := node.plugins.manager
	report(protocol.InstallationPhasePreparing, nil)
	var info protocol.PluginInfo
	var err error
	switch input := request.Command.Data.(type) {
	case *protocol.CommandInstallMcpPayload:
		info, err = node.prepareStandaloneMCP(ctx, *input)
		if err == nil {
			prepared.mcp, err = node.declaredPluginMCP(ctx, plugins.PackageReference(info))
		}
	case *protocol.CommandSavePluginMcpPayload:
		prepared.mcp, err = node.declaredPluginMCP(ctx, input.Package)
		return prepared, err
	case *protocol.CommandSavePluginSettingsPayload:
		prepared.mcpTargets, err = node.pluginMCPTargets(ctx, input.Package)
		return prepared, err
	case *protocol.CommandInstallBundledPluginPayload:
		info, err = manager.InstallBundled(ctx, input.Name)
	case *protocol.CommandInstallPluginPayload:
		var root string
		root, err = node.workspaceRoot(ctx, input.Worktree, false)
		if err == nil {
			info, err = manager.Install(ctx, input.Path, root, input.Name)
		}
		if err == nil && input.ExpectedDigest != nil && *input.ExpectedDigest != info.Summary.Digest {
			err = &protocol.Fault{Code: protocol.ErrorCodeRevisionConflict, Message: "plugin package changed during installation"}
		}
		info.Origin = &protocol.PluginOrigin{Kind: "worktree", Data: protocol.PluginOriginWorktreePayload{Worktree: input.Worktree, Path: input.Path}}
	case *protocol.CommandInstallPluginSourcePayload:
		var selection protocol.PluginCatalogSelection
		selection, err = manager.InspectRepository(ctx, protocol.PluginSkillsSource{Repository: input.Source.Repository, GitRef: &input.Source.Commit, Path: &input.Path})
		if err == nil && selection.Source.Commit != input.Source.Commit {
			err = &protocol.Fault{Code: protocol.ErrorCodeRevisionConflict, Message: "plugin repository commit changed"}
		}
		if err == nil {
			selection.Source = input.Source
			if selection.Info.Summary.Name != input.Name {
				err = &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "plugin package name differs from the selected package"}
			} else {
				info, err = manager.InstallRepository(ctx, selection)
			}
		}
	case *protocol.CommandInstallPluginUploadPayload:
		err = node.transfers.UsePluginUpload(ctx, caller, input.Stream, true, func(reader io.ReaderAt, size int64) error {
			var err error
			info, err = manager.InstallArchive(ctx, reader, size, input.Name)
			return err
		})
	case *protocol.CommandInstallSkillPayload:
		info, err = manager.InstallRepositorySkill(ctx, input.Source, input.Path, input.Name)
	case *protocol.CommandInstallRegistrySkillPayload:
		info, err = manager.InstallRegistry(ctx, input.Source, input.Name)
	case *protocol.CommandInstallSkillUploadPayload:
		var format protocol.PluginSkillsArchiveFormat
		format, err = node.transfers.PluginFormat(caller, input.Stream)
		if err == nil {
			err = node.transfers.UsePluginUpload(ctx, caller, input.Stream, false, func(reader io.ReaderAt, size int64) error {
				var err error
				info, err = manager.InstallSkillUpload(ctx, reader, size, format, input.Source, input.Name)
				return err
			})
		}
	default:
		if request.Command.Kind == "generate_plugin_text" {
			output, err := node.generatePluginText(ctx, request)
			prepared.output = &output
			return prepared, err
		}
		if request.Command.Kind == "call_plugin" {
			output, err := node.pluginCallback(ctx, caller, request, false)
			prepared.output = &output
			return prepared, err
		}
		if request.Command.Kind == "request_plugin_http" {
			input := request.Command.Data.(*protocol.PluginHttpRequest)
			output, err := node.pluginHTTP(ctx, request, *input)
			prepared.output = &output
			return prepared, err
		}
		if request.Command.Kind == "cancel_plugin_call" {
			output, err := node.readPluginCommand(ctx, caller, request)
			prepared.output = &output
			return prepared, err
		}
		return prepared, nil
	}
	if err == nil {
		prepared.info = &info
		report(protocol.InstallationPhaseValidating, nil)
		prepared.release, err = node.preparePackagePython(ctx, info)
	}
	return prepared, err
}

func (node *Node) executePlugin(ctx context.Context, caller protocol.NodeID, request protocol.Request, report installations.Report) link.Response {
	var prepared pluginPreparation
	var operationErr error
	if request.Plugin != nil {
		operationErr = node.validatePluginRequest(ctx, caller, request)
	}
	if operationErr == nil {
		prepared, operationErr = node.preparePluginCommand(ctx, caller, request, report)
	}
	if prepared.release != nil {
		defer prepared.release()
	}
	recording := context.WithoutCancel(node.ctx)
	var output protocol.Output
	var changes []protocol.Event
	err := node.profile.Store.Node.Write(recording, func(db *sql.Conn) error {
		if _, err := db.ExecContext(recording, "SAVEPOINT sailry_plugin_command"); err != nil {
			return err
		}
		if operationErr == nil && request.Plugin != nil && prepared.output == nil {
			operationErr = node.validatePluginRequestDB(recording, db, caller, request)
		}
		if operationErr == nil {
			output, changes, operationErr = node.mutatePluginCommand(recording, db, caller, request, prepared)
		}
		if operationErr != nil {
			changes = nil
			if _, err := db.ExecContext(recording, "ROLLBACK TO sailry_plugin_command"); err != nil {
				return err
			}
		}
		if _, err := db.ExecContext(recording, "RELEASE sailry_plugin_command"); err != nil {
			return err
		}
		if _, err := store.RecordEvents(recording, db, node.ID(), changes); err != nil {
			return err
		}
		return admission.Complete(recording, db, caller, request.ID, response(output, operationErr))
	})
	if err != nil {
		node.authCommitted(caller, request, err)
		return response(protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "plugin operation result could not be recorded; inspect the resource before retrying"})
	}
	node.authCommitted(caller, request, operationErr)
	node.pluginCommitted(changes)
	if output.Kind == "python_settings" && operationErr == nil {
		if value, ok := dispatchPayload[protocol.PythonInfo](output.Data); ok {
			_ = node.plugins.python.Configure(value.Settings.Index)
		}
	}
	return response(output, operationErr)
}

func (node *Node) mutatePluginCommand(ctx context.Context, db *sql.Conn, caller protocol.NodeID, request protocol.Request, prepared pluginPreparation) (protocol.Output, []protocol.Event, error) {
	if output, changes, handled, err := node.mutateAuthCommand(ctx, db, caller, request); handled {
		return output, changes, err
	}
	if prepared.output != nil {
		return *prepared.output, nil, nil
	}
	command := request.Command
	if prepared.info != nil {
		expected := uint64(0)
		switch input := command.Data.(type) {
		case *protocol.CommandInstallBundledPluginPayload:
			expected = input.ExpectedRevision
		case *protocol.CommandInstallPluginPayload:
			expected = input.ExpectedRevision
		case *protocol.CommandInstallPluginSourcePayload:
			expected = input.ExpectedRevision
		case *protocol.CommandInstallPluginUploadPayload:
			expected = input.ExpectedRevision
		case *protocol.CommandInstallSkillPayload:
			expected = input.ExpectedRevision
		case *protocol.CommandInstallRegistrySkillPayload:
			expected = input.ExpectedRevision
		case *protocol.CommandInstallSkillUploadPayload:
			expected = input.ExpectedRevision
		case *protocol.CommandInstallMcpPayload:
			expected = input.ExpectedRevision
		}
		info, err := store.InstallPlugin(ctx, db, *prepared.info, expected, true)
		if input, ok := command.Data.(*protocol.CommandInstallMcpPayload); ok && err == nil {
			info, err = store.SaveStandaloneMCPSettings(ctx, db, info, input.Secrets, prepared.mcp)
		}
		return protocol.Output{Kind: "plugin", Data: info}, []protocol.Event{{Kind: "plugin_changed", Data: info.Summary}}, err
	}
	switch input := command.Data.(type) {
	case *protocol.CommandSavePluginMcpPayload:
		info, err := store.SavePluginMCP(ctx, db, *input, prepared.mcp)
		return protocol.Output{Kind: "plugin", Data: info}, []protocol.Event{{Kind: "plugin_changed", Data: info.Summary}}, err
	case *protocol.CommandSetPluginMcpAllowedPayload:
		info, err := store.SetPluginMCPAllowed(ctx, db, *input)
		return protocol.Output{Kind: "plugin", Data: info}, []protocol.Event{{Kind: "plugin_changed", Data: info.Summary}}, err
	case *protocol.PythonSettings:
		if request.Plugin != nil {
			return protocol.Output{}, nil, &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "Python settings require a controller"}
		}
		return store.SavePythonSettings(ctx, db, *input)
	case *protocol.CommandSetPluginEnabledPayload:
		info, err := store.SetPluginEnabled(ctx, db, input.Name, input.ExpectedRevision, input.Enabled)
		return protocol.Output{Kind: "plugin", Data: info}, []protocol.Event{{Kind: "plugin_changed", Data: info.Summary}}, err
	case *protocol.CommandRemovePluginPayload:
		_, err := store.RemovePlugin(ctx, db, input.Name, input.ExpectedRevision)
		summaries, readErr := store.ReadPlugins(ctx, db)
		if err == nil {
			err = readErr
		}
		return protocol.Output{Kind: "plugins", Data: summaries}, []protocol.Event{{Kind: "plugin_removed", Data: protocol.EventPluginRemovedPayload{Name: input.Name}}}, err
	case *protocol.CommandSavePluginSettingsPayload:
		value, err := store.SavePluginSettings(ctx, db, *input, prepared.mcpTargets)
		var changes []protocol.Event
		if err == nil {
			info, readErr := store.ReadPlugin(ctx, db, input.Package.Name)
			err = readErr
			if info != nil {
				changes = append(changes, protocol.Event{Kind: "plugin_changed", Data: info.Summary})
			}
		}
		if err == nil {
			dispatch, dispatchErr := store.DispatchSettingsChanged(ctx, db, value.Package, time.Now().UnixMilli())
			changes, err = append(changes, dispatch...), dispatchErr
		}
		return protocol.Output{Kind: "plugin_settings", Data: value}, changes, err
	case *protocol.CommandDispatchPayload:
		if request.Plugin == nil {
			return store.ExecuteDispatch(ctx, db, node.ID(), input.Package, input.Action, time.Now().UnixMilli())
		}
		return store.ExecuteDispatchAdmitted(ctx, db, node.ID(), input.Package, input.Action, time.Now().UnixMilli())
	case *protocol.CommandStopDispatchTurnPayload:
		turn, err := store.ReadDispatchTurn(ctx, db, node.ID(), input.Package.Name, input.Job)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		request.Command = protocol.Command{Kind: "stop_turn", Data: &protocol.CommandStopTurnPayload{Turn: turn}}
		return store.ExecuteAgentCommand(ctx, db, node.ID(), node.profile.Ownership.Path, caller, request, time.Now())
	default:
		return store.ExecutePluginStorageCommand(ctx, db, node.ID(), node.profile.Ownership.Path, caller, request)
	}
}

func (node *Node) readPluginCommand(ctx context.Context, caller protocol.NodeID, request protocol.Request) (protocol.Output, error) {
	if input, ok := request.Command.Data.(*protocol.CommandReadPluginViewPayload); ok {
		return node.readPluginView(ctx, *input)
	}
	if request.Command.Kind == "read_plugin_connection" {
		return node.readPluginConnection(ctx, caller, request)
	}
	if request.Command.Kind == "list_plugin_models" || request.Command.Kind == "resolve_plugin_model" {
		return node.pluginModelCatalog(ctx, request)
	}
	if output, handled, err := node.prepareAuthCommand(ctx, caller, request); handled {
		return output, err
	}
	if request.Command.Kind == "read_plugin_mcp" {
		return node.readPluginMCP(ctx, request)
	}
	if request.Command.Kind == "check_plugin_mcp" {
		return node.checkPluginMCP(ctx, request)
	}
	if request.Command.Kind == "read_plugin_callback" {
		return node.pluginCallback(ctx, caller, request, true)
	}
	switch input := request.Command.Data.(type) {
	case *protocol.CommandReadPluginSettingsPayload:
		targets, err := node.pluginMCPTargets(ctx, input.Package)
		if err != nil {
			return protocol.Output{}, err
		}
		var value protocol.PluginSettingsState
		err = node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			var err error
			value, err = store.ReadPluginSettings(ctx, db, input.Package, targets)
			return err
		})
		return protocol.Output{Kind: "plugin_settings", Data: value}, err
	case *protocol.CommandReadPluginSecretPayload:
		return node.readPluginSecret(ctx, request, *input)
	case *protocol.PluginUploadSpec:
		value, err := node.transfers.UploadPlugin(ctx, caller, *input)
		return protocol.Output{Kind: "plugin_upload", Data: value}, err
	case *protocol.CommandReadInstallationsPayload:
		value, err := node.plugins.installations.Read(caller, input.Requests, request.Plugin)
		return protocol.Output{Kind: "installations", Data: value}, err
	case *protocol.CommandDismissInstallationPayload:
		err := node.plugins.installations.Dismiss(caller, input.Request, request.Plugin)
		return protocol.Output{Kind: "installation_dismissed", Data: protocol.OutputInstallationDismissedPayload{Request: input.Request}}, err
	case *protocol.CommandCancelPluginCallPayload:
		node.plugins.mu.Lock()
		stopping := false
		for _, invocation := range node.plugins.invocations {
			if invocation.caller == caller && invocation.request == input.Request && request.Plugin != nil && invocation.scope.Package == request.Plugin.Package {
				invocation.cancel()
				stopping = true
			}
		}
		node.plugins.mu.Unlock()
		if !stopping {
			return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeNotFound, Message: "plugin callback is not running"}
		}
		return protocol.Output{Kind: "plugin_call_stopping", Data: protocol.OutputPluginCallStoppingPayload{Request: input.Request}}, nil
	case *protocol.CommandSearchPluginCatalogPayload:
		value, err := node.plugins.manager.Catalog(ctx, input.Source, input.Query, input.Page, input.Force)
		return protocol.Output{Kind: "plugin_catalog", Data: value}, err
	case *protocol.CommandSearchSkillCatalogPayload:
		value, err := node.plugins.manager.SkillCatalog(ctx, input.Source, input.Query, input.Page, input.Force)
		return protocol.Output{Kind: "skill_catalog", Data: value}, err
	case *protocol.CommandInspectPluginSourcePayload:
		value, err := node.plugins.manager.InspectRepository(ctx, input.Source)
		return protocol.Output{Kind: "plugin_source", Data: value}, err
	case *protocol.CommandReadCatalogPluginPayload:
		value, err := node.plugins.manager.CatalogSource(ctx, input.Source, input.ID)
		return protocol.Output{Kind: "plugin_repository", Data: value}, err
	case *protocol.CommandReadCatalogPluginInfoPayload:
		var info protocol.PluginInfo
		var err error
		if input.Bundled {
			info, err = node.plugins.manager.InspectBundled(ctx, input.ID)
		} else {
			source, sourceErr := node.plugins.manager.CatalogSource(ctx, input.Source, input.ID)
			err = sourceErr
			if err == nil {
				selection, sourceErr := node.plugins.manager.InspectRepository(ctx, source)
				info, err = selection.Info, sourceErr
			}
		}
		return protocol.Output{Kind: "catalog_details", Data: protocol.PluginCatalogDetails{Kind: "plugin", Data: info}}, err
	case *protocol.CommandDiscoverSkillsPayload:
		value, err := node.plugins.manager.DiscoverRepositorySkills(ctx, input.Source)
		return protocol.Output{Kind: "skill_discovery", Data: value}, err
	case *protocol.CommandDiscoverRegistrySkillPayload:
		value, err := node.plugins.manager.DiscoverRegistry(ctx, input.Source)
		return protocol.Output{Kind: "registry_skill_discovery", Data: value}, err
	case *protocol.CommandReadSkillDocumentPayload:
		value, err := node.plugins.manager.ReadSkillDocument(ctx, input.Source)
		return protocol.Output{Kind: "skill_document", Data: value}, err
	case *protocol.CommandDiscoverLocalSkillsPayload:
		root, err := node.workspaceRoot(ctx, input.Worktree, false)
		if err != nil {
			return protocol.Output{}, err
		}
		data, format, kind, err := node.plugins.manager.SnapshotSkills(ctx, input.Path, root)
		if err != nil {
			return protocol.Output{}, err
		}
		upload, err := node.transfers.StagePluginUpload(ctx, caller, data)
		if err != nil {
			return protocol.Output{}, err
		}
		value, err := node.plugins.manager.DiscoverSkillImport(ctx, data, upload.Stream, format, kind)
		if err == nil {
			err = node.transfers.SetPluginFormat(caller, upload.Stream, format)
		}
		return protocol.Output{Kind: "skill_import_discovery", Data: value}, err
	case *protocol.CommandDiscoverSkillUploadPayload:
		var value protocol.PluginSkillsImportDiscovery
		err := node.transfers.UsePluginUpload(ctx, caller, input.Stream, false, func(reader io.ReaderAt, size int64) error {
			var err error
			value, err = node.plugins.manager.DiscoverSkillUpload(ctx, reader, size, input.Stream, input.Format, input.Source)
			return err
		})
		if err == nil {
			err = node.transfers.SetPluginFormat(caller, input.Stream, input.Format)
		}
		return protocol.Output{Kind: "skill_import_discovery", Data: value}, err
	case *protocol.CommandInspectPluginUploadPayload:
		var info protocol.PluginInfo
		err := node.transfers.UsePluginUpload(ctx, caller, input.Stream, false, func(reader io.ReaderAt, size int64) error {
			var err error
			info, err = node.plugins.manager.InspectArchive(ctx, reader, size, "")
			return err
		})
		return protocol.Output{Kind: "plugin", Data: info}, err
	case *protocol.CommandCheckPluginUpdatePayload:
		report, err := node.checkPluginUpdate(ctx, input.Name, input.ExpectedRevision, input.Force)
		return protocol.Output{Kind: "plugin_update", Data: report}, err
	case *protocol.CommandCheckPluginUpdatesPayload:
		value := protocol.PluginUpdatesBatch{Reports: []protocol.PluginUpdatesReport{}, Failures: []protocol.PluginUpdatesFailure{}}
		for _, item := range input.Packages {
			report, err := node.checkPluginUpdate(ctx, item.Name, item.ExpectedRevision, input.Force)
			if err == nil {
				value.Reports = append(value.Reports, report)
			} else {
				value.Failures = append(value.Failures, protocol.PluginUpdatesFailure{Name: item.Name, Error: *pluginFailure(err)})
			}
		}
		return protocol.Output{Kind: "plugin_updates", Data: value}, nil
	case *protocol.PluginHttpRequest:
		return node.pluginHTTP(ctx, request, *input)
	}
	var output protocol.Output
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		if request.Command.Kind == "read_python_settings" {
			if request.Plugin != nil {
				return &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "Python settings require a controller"}
			}
			settings, err := store.ReadPythonSettings(ctx, db)
			output = protocol.Output{Kind: "python_settings", Data: protocol.PythonInfo{Settings: settings, Version: python.Version}}
			return err
		}
		if request.Command.Kind == "dispatch" {
			input := request.Command.Data.(*protocol.CommandDispatchPayload)
			var err error
			output, _, err = store.ExecuteDispatch(ctx, db, node.ID(), input.Package, input.Action, time.Now().UnixMilli())
			return err
		}
		var err error
		output, err = store.ReadPluginCommand(ctx, db, request)
		return err
	})
	if err == nil && output.Kind == "plugin" {
		if info, ok := dispatchPayload[protocol.PluginInfo](output.Data); ok {
			err = node.projectPluginMCP(ctx, &info)
			output.Data = info
		}
	}
	return output, err
}

// readPluginSecret returns one saved secret slot to a controller's settings
// form. A slot whose declared binding or MCP destination changed since it was
// saved stays hidden.
func (node *Node) readPluginSecret(ctx context.Context, request protocol.Request, input protocol.CommandReadPluginSecretPayload) (protocol.Output, error) {
	if request.Plugin != nil {
		return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "credential contents are not exposed to plugin actions"}
	}
	targets, err := node.pluginMCPTargets(ctx, input.Package)
	if err != nil {
		return protocol.Output{}, err
	}
	var secret *protocol.Secret
	err = node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		state, err := store.ReadPluginSettings(ctx, db, input.Package, targets)
		if err != nil {
			return err
		}
		info, err := store.ReadPlugin(ctx, db, input.Package.Name)
		if err != nil {
			return err
		}
		if info == nil || info.Settings == nil || info.Settings.Properties[input.Field].Secret == nil {
			return &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "setting is not a declared secret slot"}
		}
		if !slices.Contains(state.Configured, input.Field) {
			if !slices.Contains(state.Keepable, input.Field) {
				return &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "setting binding changed"}
			}
			return nil
		}
		resolved, err := store.ResolvePluginSettings(ctx, db, input.Package)
		if err != nil {
			return err
		}
		value, saved := resolved.Secrets[input.Field]
		if !saved {
			return &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "setting binding changed"}
		}
		secret = &value
		return nil
	})
	return protocol.Output{Kind: "plugin_secret", Data: secret}, err
}

func (node *Node) checkPluginUpdate(ctx context.Context, name string, expected uint64, force bool) (protocol.PluginUpdatesReport, error) {
	var info *protocol.PluginInfo
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		info, err = store.ReadPlugin(ctx, db, name)
		return err
	})
	if err != nil {
		return protocol.PluginUpdatesReport{}, err
	}
	if info == nil {
		return protocol.PluginUpdatesReport{}, &protocol.Fault{Code: protocol.ErrorCodeNotFound, Message: "plugin is not installed"}
	}
	if info.Summary.Revision != expected {
		return protocol.PluginUpdatesReport{}, &protocol.Fault{Code: protocol.ErrorCodeRevisionConflict, Message: "plugin changed before update check"}
	}
	var root *string
	if info.Origin != nil && info.Origin.Kind == "worktree" {
		if origin, ok := dispatchPayload[protocol.PluginOriginWorktreePayload](info.Origin.Data); ok {
			value, err := node.workspaceRoot(ctx, origin.Worktree, false)
			if err != nil {
				return protocol.PluginUpdatesReport{}, err
			}
			root = &value
		}
	}
	return node.plugins.manager.CheckUpdate(ctx, *info, root, force)
}

func (node *Node) pluginHTTP(ctx context.Context, request protocol.Request, input protocol.PluginHttpRequest) (protocol.Output, error) {
	if request.Plugin == nil {
		return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "plugin provenance is required"}
	}
	ctx, release, err := node.beginPluginExecution(ctx, request)
	if err != nil {
		return protocol.Output{}, err
	}
	defer release()
	var info protocol.PluginInfo
	var settings store.ResolvedPluginSettings
	err = node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		info, err = store.ReadPluginVersion(ctx, db, request.Plugin.Package)
		if err == nil {
			settings, err = store.ResolvePluginSettings(ctx, db, request.Plugin.Package)
		}
		return err
	})
	if err != nil {
		return protocol.Output{}, err
	}
	prepared, err := plugins.PrepareHTTP(input, info.Settings, settings.Secrets)
	if err != nil {
		return protocol.Output{}, err
	}
	prepared, err = node.authorizePluginHTTP(ctx, request, info, input, prepared)
	if err != nil {
		return protocol.Output{}, err
	}
	response, err := plugins.ExecuteHTTP(ctx, prepared, authorization.HTTPClient())
	return protocol.Output{Kind: "plugin_http", Data: response}, err
}
