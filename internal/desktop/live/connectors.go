package live

import (
	"maps"
	"slices"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func connectorTypeID(reference protocol.PluginReference, connector string) string {
	return reference.Name + "/" + connector
}

func connectorMethod(method protocol.PluginConnectorKind) string {
	switch method {
	case protocol.PluginConnectorKindOauth2, protocol.PluginConnectorKindMcpOauth:
		return "connector_oauth"
	case protocol.PluginConnectorKindApplication:
		return "connector_application"
	case protocol.PluginConnectorKindLocalCli:
		return "connector_cli"
	default:
		return "connector_token"
	}
}

func (r *Runtime) initConnectorUI() {
	r.App.Settings.ConnectorActions = &settings.ConnectorActions{
		Save: r.saveConnector, Enable: r.enableConnector, Remove: r.removeConnector, Confirm: r.confirmConnector,
		Probe: r.probeConnection, Reconnect: r.reconnectMCP, InspectCLI: r.inspectConnectorCLI, SaveMCP: r.saveMCP,
		AllowMCP: r.allowMCP, Submit: r.submitConnectorLogin, Cancel: r.cancelConnectorLogin,
		Retry: func() {
			if previous := r.authorizations.connector; previous != nil {
				r.saveConnector(previous.connector, nil)
			}
		},
		ConfigureMCP: r.configureConnectorMCP,
		Help: func(id, name string) string {
			for _, field := range r.authorizations.types[id].Declaration.Fields {
				if field.Name == name && field.Help != nil {
					return *field.Help
				}
			}
			return ""
		},
	}
	r.App.Settings.Services, r.App.Settings.ConnTypes, r.App.Settings.ConnAccounts = nil, nil, nil
	r.observe(func() {
		stream, err := r.controller.Subscribe(r.ctx, protocol.Topic{Kind: "connections"})
		if err != nil {
			r.enqueue(func(c *ui.Context) { r.connectorError(c, err) })
			return
		}
		defer stream.Close()
		for {
			update, err := stream.Next(r.ctx)
			if err != nil {
				return
			}
			value, ok := detail[protocol.PluginConnectorChanges](update.Data)
			if !ok {
				continue
			}
			r.enqueue(func(*ui.Context) {
				r.authorizations.types = map[string]protocol.PluginConnectorType{}
				for _, value := range value.Types {
					r.authorizations.types[connectorTypeID(value.Package, value.Declaration.ID)] = value
				}
				r.authorizations.connections = map[string]protocol.PluginConnectorConnection{}
				for _, value := range value.Connections {
					r.authorizations.connections[connectorTypeID(value.Package, value.Connector)] = value
				}
				r.projectConnectors()
				r.readConnectorPackages()
			})
		}
	})
}

func (r *Runtime) connectorError(c *ui.Context, err error) {
	r.reportKeys(c, err, "connector_failed", "connector_conflict", "connector_unknown")
}

func (r *Runtime) readConnectorPackages() {
	summaries := r.controller.Snapshot().Plugins
	r.async(func() {
		infos := map[string]protocol.PluginInfo{}
		for _, summary := range summaries {
			output, err := r.controller.Do(r.ctx, protocol.Command{Kind: "read_plugin", Data: protocol.CommandReadPluginPayload{Name: summary.Name}})
			if err == nil {
				if info, ok := detail[protocol.PluginInfo](output.Data); ok {
					infos[summary.Name] = info
				}
			}
		}
		r.enqueue(func(*ui.Context) {
			r.authorizations.packages = infos
			r.projectConnectors()
		})
	})
}

func (r *Runtime) projectConnectors() {
	var types []viewmodel.ConnectorType
	for _, id := range slices.Sorted(maps.Keys(r.authorizations.types)) {
		value := r.authorizations.types[id]
		declaration := value.Declaration
		kind := viewmodel.ConnectorType{ID: id, Package: value.Package.Name, Name: declaration.Display.Label, Enabled: value.Enabled}
		for _, auth := range declaration.Auth {
			method := connectorMethod(protocol.PluginConnectorKind(auth.Kind))
			if !slices.Contains(kind.Methods, method) {
				kind.Methods = append(kind.Methods, method)
			}
		}
		for _, field := range declaration.Fields {
			kind.Fields = append(kind.Fields, viewmodel.ConnectorField{Name: field.Name, Title: field.Title, Secret: field.Secret, Optional: field.Optional, Help: field.Help != nil})
		}
		types = append(types, kind)
	}
	var services []viewmodel.ConnectorService
	covered := map[string]map[string]bool{}
	for _, id := range slices.Sorted(maps.Keys(r.authorizations.connections)) {
		connection := r.authorizations.connections[id]
		value, exists := r.authorizations.types[id]
		name := connection.Connector
		if exists {
			name = value.Declaration.Display.Label
		}
		service := viewmodel.ConnectorService{
			ID: id, Type: id, Name: name, Revision: connection.Revision, Method: connectorMethod(connection.Method),
			Account: true, AccountOn: connection.Enabled, Fields: maps.Clone(connection.Fields),
		}
		if connection.Identity != nil {
			service.Identity = connection.Identity.Name
		}
		service.State = map[protocol.PluginConnectorState]viewmodel.ConnState{
			protocol.PluginConnectorStateNotConnected: viewmodel.Unconnected, protocol.PluginConnectorStateConnecting: viewmodel.Connecting,
			protocol.PluginConnectorStateConnected: viewmodel.Connected, protocol.PluginConnectorStateExpired: viewmodel.Expired,
			protocol.PluginConnectorStateNeedsConfirmation: viewmodel.NeedsConfirmation, protocol.PluginConnectorStateFailed: viewmodel.ConnFailed,
		}[connection.State]
		info := r.authorizations.packages[connection.Package.Name]
		for _, injection := range value.Declaration.Inject {
			if injection.Server == nil {
				continue
			}
			if covered[connection.Package.Name] == nil {
				covered[connection.Package.Name] = map[string]bool{}
			}
			if covered[connection.Package.Name][*injection.Server] {
				continue
			}
			for _, server := range info.Mcp {
				if server.Name == *injection.Server {
					service.Servers = append(service.Servers, connectorServer(server))
					covered[connection.Package.Name][server.Name] = true
				}
			}
		}
		services = append(services, service)
	}
	for _, name := range slices.Sorted(maps.Keys(r.authorizations.packages)) {
		info := r.authorizations.packages[name]
		var servers []viewmodel.ConnectorServer
		for _, server := range info.Mcp {
			if !covered[name][server.Name] {
				servers = append(servers, connectorServer(server))
			}
		}
		if len(servers) == 0 {
			continue
		}
		service := viewmodel.ConnectorService{ID: "mcp:" + name, Type: connectorTypeID(protocol.PluginReference{Name: name}, "server"), Name: strings.TrimPrefix(name, "mcp-"), Revision: info.Summary.Revision, Manual: info.McpSource != nil, Servers: servers}
		if info.McpSource != nil {
			body, _ := detail[protocol.PluginMcpDefinitionStreamableHttpPayload](info.McpSource.Data)
			if info.McpSource.Kind == "stdio" {
				value, _ := detail[protocol.PluginMcpDefinitionStdioPayload](info.McpSource.Data)
				service.Servers[0].Command, service.Servers[0].Args = value.Command, value.Args
				for _, name := range value.Env {
					service.Servers[0].Env = append(service.Servers[0].Env, viewmodel.EnvVar{Key: name})
				}
			} else {
				service.Servers[0].URL = body.Url
				for _, name := range body.Headers {
					service.Servers[0].Env = append(service.Servers[0].Env, viewmodel.EnvVar{Key: name})
				}
			}
		}
		services = append(services, service)
	}
	r.App.Settings.ConnTypes, r.App.Settings.Services = types, services
}

func connectorServer(value protocol.PluginMcp) viewmodel.ConnectorServer {
	server := viewmodel.ConnectorServer{Name: value.Name, Allowed: value.Allowed, State: viewmodel.ServerDown}
	switch value.Transport {
	case protocol.PluginMcpTransportStreamableHttp:
		server.Transport = 1
	case protocol.PluginMcpTransportSse:
		server.Transport = 2
	}
	if value.Availability == protocol.PluginMcpAvailabilityConnectionRequired || value.Availability == protocol.PluginMcpAvailabilityAuthorizationExpired || value.Availability == protocol.PluginMcpAvailabilityConsentRequired {
		server.State = viewmodel.ServerAuth
	}
	if runtime := value.Runtime; runtime != nil {
		server.Tools = int(runtime.Tools)
		server.Updated = time.UnixMilli(int64(runtime.UpdatedAtMs)).Local().Format("2006-01-02 15:04:05")
		switch runtime.State {
		case protocol.PluginMcpRuntimeStateConnecting:
			server.State = viewmodel.ServerStarting
		case protocol.PluginMcpRuntimeStateConnected:
			server.State = viewmodel.ServerUp
		case protocol.PluginMcpRuntimeStateFailed:
			server.State = viewmodel.ServerDown
		}
		if runtime.Fault != nil {
			server.Error = runtime.Fault.Message
		}
	}
	return server
}

func (r *Runtime) connectorCommand(command protocol.Command, done func(bool)) {
	r.execute(command, func(c *ui.Context, output protocol.Output, err error) {
		r.connectorError(c, err)
		if done != nil {
			done(err == nil)
		}
		if err == nil {
			if value, ok := detail[protocol.PluginConnectorConnection](output.Data); ok {
				r.authorizations.connections[connectorTypeID(value.Package, value.Connector)] = value
			}
			r.projectConnectors()
			r.readConnectorPackages()
		}
	})
}

func (r *Runtime) enableConnector(id string, enabled bool) {
	connection, exists := r.authorizations.connections[id]
	if !exists {
		return
	}
	r.connectorCommand(protocol.Command{Kind: "set_connection_enabled", Data: protocol.CommandSetConnectionEnabledPayload{Connection: connection.ID, ExpectedRevision: connection.Revision, Enabled: enabled}}, nil)
}

func (r *Runtime) removeConnector(id string) {
	if connection, exists := r.authorizations.connections[id]; exists {
		r.connectorCommand(protocol.Command{Kind: "remove_connection", Data: protocol.CommandRemoveConnectionPayload{Connection: connection.ID, ExpectedRevision: connection.Revision}}, func(ok bool) {
			if ok {
				delete(r.authorizations.connections, id)
				r.enqueue(func(c *ui.Context) { kit.Toast(c, kit.Done, i18n.T(c, "connector_removed")) })
			}
		})
		return
	}
	if name, exists := strings.CutPrefix(id, "mcp:"); exists {
		info := r.authorizations.packages[name]
		r.connectorCommand(protocol.Command{Kind: "remove_plugin", Data: protocol.CommandRemovePluginPayload{Name: name, ExpectedRevision: info.Summary.Revision}}, func(ok bool) {
			if ok {
				delete(r.authorizations.packages, name)
				r.enqueue(func(c *ui.Context) { kit.Toast(c, kit.Done, i18n.T(c, "connector_removed")) })
			}
		})
	}
}

func (r *Runtime) confirmConnector(id string) {
	if connection, exists := r.authorizations.connections[id]; exists {
		r.enqueue(func(c *ui.Context) {
			var changes []string
			if connection.Consent != nil {
				for _, change := range connection.Consent.Changes {
					changes = append(changes, change.Kind)
				}
			}
			r.App.Settings.ConfirmConnectorConsent(connection.Package.Name, changes, func(accept bool) {
				r.connectorCommand(protocol.Command{Kind: "confirm_connector_consent", Data: protocol.CommandConfirmConnectorConsentPayload{Connection: connection.ID, ExpectedRevision: connection.Revision, Accept: accept}}, nil)
			})
		})
	}
}

func (r *Runtime) configureConnectorMCP(id string) {
	info, exists := r.connectorPackage(id)
	if !exists {
		return
	}
	if info.Settings != nil {
		r.authorizations.mcpConfiguring = info.Summary.Name
		r.configurePackage(info.Summary.Name)
		return
	}
	reference := protocol.PluginReference{Name: info.Summary.Name, Digest: info.Summary.Digest, SettingsRevision: info.Summary.SettingsRevision}
	r.execute(protocol.Command{Kind: "read_plugin_mcp", Data: protocol.CommandReadPluginMcpPayload{Package: reference}}, func(c *ui.Context, output protocol.Output, err error) {
		r.connectorError(c, err)
		if err != nil {
			return
		}
		state, ok := detail[protocol.PluginMcpState](output.Data)
		if !ok || state.Package != reference {
			return
		}
		names := slices.Sorted(maps.Keys(state.Configuration.Servers))
		if len(names) == 0 {
			return
		}
		name := names[0]
		server := state.Configuration.Servers[name]
		view := viewmodel.ConnectorServer{Name: name}
		switch server.Kind {
		case "stdio":
			value, _ := detail[protocol.PluginMcpServerStdioPayload](server.Data)
			view.Command, view.Args = value.Command, slices.Clone(value.Args)
			for _, field := range slices.Sorted(maps.Keys(value.Env)) {
				view.Env = append(view.Env, viewmodel.EnvVar{Key: field})
			}
		case "sse":
			value, _ := detail[protocol.PluginMcpServerSsePayload](server.Data)
			view.Transport, view.URL = 2, value.Url
			for _, field := range slices.Sorted(maps.Keys(value.Headers)) {
				view.Env = append(view.Env, viewmodel.EnvVar{Key: field})
			}
		default:
			value, _ := detail[protocol.PluginMcpServerHttpPayload](server.Data)
			view.Transport, view.URL = 1, value.Url
			for _, field := range slices.Sorted(maps.Keys(value.Headers)) {
				view.Env = append(view.Env, viewmodel.EnvVar{Key: field})
			}
		}
		r.authorizations.mcpDrafts[id] = state
		r.App.Settings.OpenMCPConfiguration(viewmodel.ConnectorService{ID: id, Revision: info.Summary.Revision, Name: name, Servers: []viewmodel.ConnectorServer{view}})
	})
}

func (r *Runtime) probeConnection(id string) {
	if connection, exists := r.authorizations.connections[id]; exists {
		r.connectorCommand(protocol.Command{Kind: "probe_connection", Data: protocol.CommandProbeConnectionPayload{Connection: connection.ID}}, func(ok bool) {
			if ok {
				r.enqueue(func(c *ui.Context) { kit.Toast(c, kit.Done, i18n.T(c, "connector_checked")) })
			}
		})
	}
}

func (r *Runtime) connectorPackage(id string) (protocol.PluginInfo, bool) {
	if value, exists := r.authorizations.connections[id]; exists {
		info, ok := r.authorizations.packages[value.Package.Name]
		return info, ok
	}
	name := strings.TrimPrefix(id, "mcp:")
	info, exists := r.authorizations.packages[name]
	return info, exists
}

func (r *Runtime) reconnectMCP(id string) {
	info, exists := r.connectorPackage(id)
	if !exists {
		return
	}
	for _, server := range info.Mcp {
		if !server.Allowed {
			continue
		}
		r.execute(protocol.Command{Kind: "check_plugin_mcp", Data: protocol.CommandCheckPluginMcpPayload{Package: protocol.PluginReference{Name: info.Summary.Name, Digest: info.Summary.Digest, SettingsRevision: info.Summary.SettingsRevision}, Server: server.Name}}, func(c *ui.Context, output protocol.Output, err error) {
			r.connectorError(c, err)
			if err == nil {
				result, _ := detail[protocol.PluginMcpCheck](output.Data)
				kit.Toast(c, kit.Done, i18n.T(c, "connector_mcp_reconnected", "n", len(result.Tools)))
			}
			r.readConnectorPackages()
		})
	}
}

func (r *Runtime) allowMCP(id, server string, allowed bool) {
	info, exists := r.connectorPackage(id)
	if !exists {
		return
	}
	r.connectorCommand(protocol.Command{Kind: "set_plugin_mcp_allowed", Data: protocol.CommandSetPluginMcpAllowedPayload{Package: protocol.PluginReference{Name: info.Summary.Name, Digest: info.Summary.Digest, SettingsRevision: info.Summary.SettingsRevision}, Server: server, Allowed: allowed}}, func(ok bool) {
		if ok {
			r.enqueue(func(c *ui.Context) { kit.Toast(c, kit.Done, i18n.T(c, "connector_saved")) })
		}
	})
}

func (r *Runtime) inspectConnectorCLI(id string, done func([]string)) {
	value, exists := r.authorizations.types[id]
	if !exists {
		done(nil)
		return
	}
	r.execute(protocol.Command{Kind: "inspect_connector_cli", Data: protocol.CommandInspectConnectorCliPayload{Package: value.Package, Connector: value.Declaration.ID}}, func(c *ui.Context, output protocol.Output, err error) {
		if err != nil {
			r.connectorError(c, err)
			done(nil)
			return
		}
		identity, ok := detail[protocol.PluginConnectorIdentity](output.Data)
		if ok {
			done([]string{identity.Name})
		} else {
			done(nil)
		}
	})
}

func (r *Runtime) saveMCP(draft settings.MCPDraft, done func(bool)) {
	name := "mcp-" + strings.TrimPrefix(draft.Name, "mcp-")
	if draft.Service != "" {
		info, exists := r.connectorPackage(draft.Service)
		if !exists {
			done(false)
			return
		}
		name = info.Summary.Name
		if info.McpSource == nil {
			r.savePackageMCP(draft, info, done)
			return
		}
	}
	names := []string{}
	secrets := map[string]protocol.PluginSettingsSecretUpdate{}
	for _, field := range draft.Server.Env {
		names = append(names, field.Key)
		update := protocol.PluginSettingsSecretUpdate{Kind: "replace", Data: protocol.NewSecret(field.Value)}
		if field.Value == "" && draft.Service != "" {
			update = protocol.PluginSettingsSecretUpdate{Kind: "keep"}
		}
		secrets[field.Key] = update
	}
	definition := protocol.PluginMcpDefinition{Kind: "stdio", Data: protocol.PluginMcpDefinitionStdioPayload{Command: draft.Server.Command, Args: draft.Server.Args, Env: names}}
	if draft.Server.Transport == 1 {
		definition = protocol.PluginMcpDefinition{Kind: "streamable-http", Data: protocol.PluginMcpDefinitionStreamableHttpPayload{Url: draft.Server.URL, Headers: names}}
	} else if draft.Server.Transport == 2 {
		definition = protocol.PluginMcpDefinition{Kind: "sse", Data: protocol.PluginMcpDefinitionSsePayload{Url: draft.Server.URL, Headers: names}}
	}
	r.execute(protocol.Command{Kind: "install_mcp", Data: protocol.CommandInstallMcpPayload{Name: name, ExpectedRevision: draft.Revision, Definition: definition, Secrets: secrets}}, func(c *ui.Context, output protocol.Output, err error) {
		r.connectorError(c, err)
		done(err == nil)
		if err == nil {
			kit.Toast(c, kit.Done, i18n.T(c, "mcp_configuration_saved"))
			if info, ok := detail[protocol.PluginInfo](output.Data); ok {
				r.authorizations.packages[name] = info
				r.reconnectMCP("mcp:" + name)
			}
			r.readConnectorPackages()
		}
	})
}

func (r *Runtime) savePackageMCP(draft settings.MCPDraft, info protocol.PluginInfo, done func(bool)) {
	state, exists := r.authorizations.mcpDrafts[draft.Service]
	if !exists || info.Summary.Revision != draft.Revision {
		done(false)
		r.enqueue(func(c *ui.Context) {
			r.connectorError(c, &protocol.Fault{Code: protocol.ErrorCodeRevisionConflict, Message: "MCP configuration changed while editing"})
		})
		return
	}
	server, exists := state.Configuration.Servers[draft.Server.Name]
	if !exists {
		done(false)
		return
	}
	merge := func(previous map[string]string) map[string]string {
		fields := map[string]string{}
		for _, field := range draft.Server.Env {
			fields[field.Key] = field.Value
			if field.Value == "" {
				fields[field.Key] = previous[field.Key]
			}
		}
		return fields
	}
	switch server.Kind {
	case "stdio":
		value, _ := detail[protocol.PluginMcpServerStdioPayload](server.Data)
		value.Command, value.Args, value.Env = draft.Server.Command, slices.Clone(draft.Server.Args), merge(value.Env)
		server.Data = value
	case "sse":
		value, _ := detail[protocol.PluginMcpServerSsePayload](server.Data)
		value.Url, value.Headers = draft.Server.URL, merge(value.Headers)
		server.Data = value
	default:
		value, _ := detail[protocol.PluginMcpServerHttpPayload](server.Data)
		value.Url, value.Headers = draft.Server.URL, merge(value.Headers)
		server.Data = value
	}
	configuration := state.Configuration
	configuration.Servers = maps.Clone(configuration.Servers)
	configuration.Servers[draft.Server.Name] = server
	r.execute(protocol.Command{Kind: "save_plugin_mcp", Data: protocol.CommandSavePluginMcpPayload{Package: state.Package, Configuration: configuration}}, func(c *ui.Context, output protocol.Output, err error) {
		r.connectorError(c, err)
		done(err == nil)
		if err == nil {
			delete(r.authorizations.mcpDrafts, draft.Service)
			if updated, ok := detail[protocol.PluginInfo](output.Data); ok {
				r.authorizations.packages[updated.Summary.Name] = updated
			}
			kit.Toast(c, kit.Done, i18n.T(c, "mcp_configuration_saved"))
			r.reconnectMCP(draft.Service)
			r.readConnectorPackages()
		}
	})
}
