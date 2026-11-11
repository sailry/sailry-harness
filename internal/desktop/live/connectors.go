package live

import (
	"encoding/json"
	"errors"
	"maps"
	"slices"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
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

// connectorError reports a connector command's error: a failure with the
// host's reason under it, a conflict or an uncertain outcome.
func (r *Runtime) connectorError(c *ui.Context, err error) {
	if reason := faultMessage(err); reason != "" && connectorOutcome(err) == kit.Failed {
		kit.Error(c, i18n.T(c, "connector_failed"), reason)
		return
	}
	r.reportKeys(c, err, "connector_failed", "connector_conflict", "connector_unknown")
}

// readConnectorPackages reads every installed package once per pass. A call
// during a pass asks for one more pass instead of a concurrent one.
func (r *Runtime) readConnectorPackages() {
	state := r.authorizations
	if state.packagesReading {
		state.packagesStale = true
		return
	}
	state.packagesReading = true
	summaries, known := r.snapshot().Plugins, maps.Clone(state.packages)
	r.async(func() {
		infos := map[string]protocol.PluginInfo{}
		var failed error
		for _, summary := range summaries {
			output, err := r.controller.Do(r.ctx, protocol.Command{Kind: "read_plugin", Data: protocol.CommandReadPluginPayload{Name: summary.Name}})
			if info, ok := detail[protocol.PluginInfo](output.Data); err == nil && ok {
				infos[summary.Name] = info
			} else if err != nil {
				// A failed read keeps what was shown; a package removed meanwhile is gone.
				if info, shown := known[summary.Name]; shown && faultCode(err) != protocol.ErrorCodeNotFound {
					infos[summary.Name] = info
				}
				if failed == nil && faultCode(err) != protocol.ErrorCodeNotFound {
					failed = err
				}
			}
		}
		r.enqueue(func(c *ui.Context) {
			state.packagesReading = false
			state.packages = infos
			r.projectConnectors()
			if failed != nil {
				r.connectorError(c, failed)
			}
			if state.packagesStale {
				state.packagesStale = false
				r.readConnectorPackages()
			}
		})
	})
}

func (r *Runtime) projectConnectors() {
	var types []viewmodel.ConnectorType
	for _, id := range slices.Sorted(maps.Keys(r.authorizations.types)) {
		value := r.authorizations.types[id]
		declaration := value.Declaration
		kind := viewmodel.ConnectorType{ID: id, Package: value.Package.Name, Name: declaration.Display.Label, Enabled: value.Enabled, Icon: pluginIconSource(declaration.Display.Icon)}
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
		icon := ""
		if exists {
			name, icon = value.Declaration.Display.Label, pluginIconSource(value.Declaration.Display.Icon)
		}
		service := viewmodel.ConnectorService{
			ID: id, Type: id, Name: name, Icon: icon, Revision: connection.Revision, Method: connectorMethod(connection.Method),
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
					service.Servers = append(service.Servers, r.connectorServer(connection.Package.Name, server))
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
				servers = append(servers, r.connectorServer(name, server))
			}
		}
		if len(servers) == 0 {
			continue
		}
		// The service's type is its package's connector, so its credentials
		// open that connector's editor; a package without one has none.
		kind := connectorTypeID(protocol.PluginReference{Name: name}, "server")
		for _, id := range slices.Sorted(maps.Keys(r.authorizations.types)) {
			if r.authorizations.types[id].Package.Name == name {
				kind = id
				break
			}
		}
		service := viewmodel.ConnectorService{ID: "mcp:" + name, Type: kind, Name: strings.TrimPrefix(name, "mcp-"), Revision: info.Summary.Revision, Manual: info.McpSource != nil, Servers: servers}
		if info.Extension != nil && info.Extension.Display != nil {
			service.Icon = pluginIconSource(info.Extension.Display.Icon)
		}
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

// connectorProblems are the availabilities that keep a server from
// connecting, as the status each shows.
var connectorProblems = map[protocol.PluginMcpAvailability]string{
	protocol.PluginMcpAvailabilityPackageDisabled:       "connector_package_disabled",
	protocol.PluginMcpAvailabilityServerDisabled:        "connector_server_disabled",
	protocol.PluginMcpAvailabilityConfigurationRequired: "connector_configuration_required",
	protocol.PluginMcpAvailabilityConnectionDisabled:    "connector_account_disabled",
	protocol.PluginMcpAvailabilityAuthorizationExpired:  "connector_expired",
	protocol.PluginMcpAvailabilityConsentRequired:       "connector_confirmation",
	protocol.PluginMcpAvailabilityUnavailable:           "connector_unavailable",
}

// mcpFailure is the reason the last check of a server failed, for the package
// content and settings it checked.
type mcpFailure struct {
	digest   string
	settings uint64
	reason   string
}

// connectorServer is the server with a check of it in flight shown as
// connecting. The host keeps no state for a server that never connected, so
// one whose last check failed shows that failure until the package changes
// or a check succeeds.
func (r *Runtime) connectorServer(name string, value protocol.PluginMcp) viewmodel.ConnectorServer {
	server := connectorServer(value)
	if server.Problem != "" || server.State == viewmodel.ServerAuth {
		return server
	}
	key := name + "\x00" + value.Name
	summary := r.authorizations.packages[name].Summary
	if failure, failed := r.authorizations.mcpFailed[key]; failed && server.State == viewmodel.ServerIdle && failure.digest == summary.Digest && failure.settings == summary.SettingsRevision {
		server.State, server.Error = viewmodel.ServerDown, failure.reason
	}
	if r.authorizations.mcpChecking[key] {
		server.State = viewmodel.ServerStarting
	}
	return server
}

// connectorServer projects a server's availability and runtime. A server that
// may connect and was never asked to is idle, not failed; one that may not
// carries the problem, and a failed one its fault's message.
func connectorServer(value protocol.PluginMcp) viewmodel.ConnectorServer {
	server := viewmodel.ConnectorServer{Name: value.Name, Allowed: value.Allowed, State: viewmodel.ServerIdle}
	switch value.Transport {
	case protocol.PluginMcpTransportStreamableHttp:
		server.Transport = 1
	case protocol.PluginMcpTransportSse:
		server.Transport = 2
	}
	server.Problem = connectorProblems[value.Availability]
	if value.Availability == protocol.PluginMcpAvailabilityConnectionRequired {
		server.State = viewmodel.ServerAuth
	}
	if runtime := value.Runtime; runtime != nil {
		server.Tools = int(runtime.Tools)
		server.Updated = time.UnixMilli(int64(runtime.UpdatedAtMs)).Local().Format("2006-01-02 15:04:05")
		if value.Availability == protocol.PluginMcpAvailabilityAvailable {
			switch runtime.State {
			case protocol.PluginMcpRuntimeStateConnecting:
				server.State = viewmodel.ServerStarting
			case protocol.PluginMcpRuntimeStateConnected:
				server.State = viewmodel.ServerUp
			case protocol.PluginMcpRuntimeStateFailed:
				server.State = viewmodel.ServerDown
			}
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

// reconnectMCP checks every server of the service that may connect. Each
// shows as connecting until its check returns, and the outcome is a toast:
// the tools found, the failure with its reason, a conflict or an uncertain
// outcome.
func (r *Runtime) reconnectMCP(id string) {
	info, exists := r.connectorPackage(id)
	if !exists {
		return
	}
	name := info.Summary.Name
	for _, server := range info.Mcp {
		if !server.Allowed || server.Availability != protocol.PluginMcpAvailabilityAvailable {
			continue
		}
		key := name + "\x00" + server.Name
		if r.authorizations.mcpChecking[key] {
			continue
		}
		r.authorizations.mcpChecking[key] = true
		command := protocol.Command{Kind: "check_plugin_mcp", Data: protocol.CommandCheckPluginMcpPayload{Package: protocol.PluginReference{Name: name, Digest: info.Summary.Digest, SettingsRevision: info.Summary.SettingsRevision}, Server: server.Name}}
		r.execute(command, func(c *ui.Context, output protocol.Output, err error) {
			delete(r.authorizations.mcpChecking, key)
			// A check changes nothing to recover: the next Reconnect is a new
			// check, not the uncertain one's recorded answer again.
			if encoded, failed := json.Marshal(command); failed == nil {
				delete(r.uncertain, "\x00"+string(encoded))
			}
			delete(r.authorizations.mcpFailed, key)
			if err == nil {
				result, _ := detail[protocol.PluginMcpCheck](output.Data)
				kit.Toast(c, kit.Done, i18n.T(c, "connector_mcp_reconnected", "n", len(result.Tools)))
			} else if reason := faultMessage(err); reason != "" && connectorOutcome(err) == kit.Failed {
				r.authorizations.mcpFailed[key] = mcpFailure{digest: info.Summary.Digest, settings: info.Summary.SettingsRevision, reason: reason}
				kit.Error(c, i18n.T(c, "connector_mcp_failed"), reason)
			} else {
				r.connectorError(c, err)
			}
			r.projectConnectors()
			r.readConnectorPackages()
		})
	}
	r.projectConnectors()
}

// connectorOutcome is how a command's error ended: a conflict and an
// uncertain outcome are not failures.
func connectorOutcome(err error) kit.Outcome {
	switch faultCode(err) {
	case protocol.ErrorCodeConflict, protocol.ErrorCodeRevisionConflict:
		return kit.Conflict
	case protocol.ErrorCodeOutcomeUnknown:
		return kit.Uncertain
	}
	return kit.Failed
}

// faultMessage is the host's reason for a fault, "" for any other error.
func faultMessage(err error) string {
	var fault *link.FaultError
	if errors.As(err, &fault) {
		return fault.Fault.Message
	}
	var pointer *protocol.Fault
	if errors.As(err, &pointer) {
		return pointer.Message
	}
	return ""
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
