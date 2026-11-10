package plugins

import (
	"context"
	"encoding/json"
	"reflect"
	"sync"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func (owner *Host) connectorModule() HostModule {
	return HostModule{Name: "sailry/connectors", Exports: []HostExport{
		hostAsync("list", owner.readOperation("list_connections", nil)),
		hostAsync("types", owner.readOperation("list_connector_types", nil)),
		hostAsync("status", func(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
			connector, err := hostValue[string](arguments, 0)
			if err != nil {
				return nil, err
			}
			body, err := owner.readData(ctx, "list_connections", nil)
			if err != nil {
				return nil, err
			}
			var rows []map[string]json.RawMessage
			if json.Unmarshal(body, &rows) != nil {
				return nil, hostInvalid("invalid connector response")
			}
			for _, row := range rows {
				if nativeProp(row, "connector", "") == connector {
					return hostJSON(row)
				}
			}
			return json.RawMessage("null"), nil
		}),
		hostSync("login", func(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
			input, err := hostValue[map[string]json.RawMessage](arguments, 0)
			if err != nil {
				return nil, err
			}
			return owner.prepareData(ctx, "start_connector_login", map[string]any{
				"package": owner.scope().Package, "connector": input["connector"],
				"connection": optionalRaw(input, "connection"), "expected_revision": optionalRaw(input, "expected_revision"),
				"method": input["method"], "input": map[string]any{"redirect": optionalRaw(input, "redirect")},
			})
		}),
		hostSync("cancel", owner.prepareOperation("cancel_connector_login", namedArguments("attempt"))),
	}}
}

func optionalRaw(values map[string]json.RawMessage, name string) json.RawMessage {
	if value, exists := values[name]; exists {
		return value
	}
	return json.RawMessage("null")
}

func (owner *Host) connectionModule() HostModule {
	return HostModule{Name: "sailry/connections", Exports: []HostExport{
		hostAsync("listDatabases", owner.readOperation("list_databases", nil)),
		hostAsync("listSsh", owner.readOperation("list_ssh", nil)),
		hostAsync("listSshTerminals", owner.readOperation("list_ssh_terminals", namedArguments("profile"))),
		hostSync("newDatabaseId", func(context.Context, []json.RawMessage) (json.RawMessage, error) {
			return hostJSON(protocol.NewDatabaseID().String())
		}),
		hostSync("newSshId", func(context.Context, []json.RawMessage) (json.RawMessage, error) {
			return hostJSON(protocol.NewSshID().String())
		}),
		hostSync("prepareDatabase", owner.prepareDatabase),
		hostSync("prepareSsh", owner.prepareSsh),
		hostSync("prepareSshTerminal", func(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
			input, err := namedArguments("profile", "expected_revision")(arguments)
			if err != nil {
				return nil, err
			}
			appearance, err := owner.terminalAppearance()
			if err != nil {
				return nil, err
			}
			values := input.(map[string]any)
			values["launch"] = map[string]any{"viewport": hostViewport(), "appearance": appearance}
			return owner.prepareData(ctx, "open_ssh_terminal", values)
		}),
		hostAsync("browseDatabase", owner.readOperation("browse_database", namedArgumentsWithDefaults(map[string]any{"database": nil}, "profile", "expected_revision", "database"))),
		hostAsync("browseSshDirectory", owner.readOperation("browse_ssh_directory", firstArgument)),
		hostAsync("requestOutcome", func(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
			identity, err := hostValue[string](arguments, 0)
			if err != nil {
				return nil, err
			}
			return owner.outcome(ctx, identity)
		}),
	}}
}

func (owner *Host) prepareDatabase(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
	input, err := hostValue[map[string]json.RawMessage](arguments, 0)
	if err != nil {
		return nil, err
	}
	profile, err := decodeHostJSON(input["profile"])
	if err != nil {
		return nil, err
	}
	if err := commandRevision(profile, "revision"); err != nil {
		return nil, err
	}
	var password *protocol.Secret
	if identity := nativeProp(input, "secret", ""); identity != "" {
		value, err := owner.native.secret(identity)
		if err != nil {
			return nil, err
		}
		password = &value
	}
	kind := "save_database"
	if nativeProp(input, "testing", false) {
		kind = "test_database"
	}
	return owner.prepareData(ctx, kind, map[string]any{"profile": profile, "expected_revision": hostAt(profile, "revision"), "password": password})
}

func (owner *Host) prepareSsh(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
	input, err := hostValue[map[string]json.RawMessage](arguments, 0)
	if err != nil {
		return nil, err
	}
	profile, err := decodeHostJSON(input["profile"])
	if err != nil {
		return nil, err
	}
	if err := commandRevision(profile, "revision"); err != nil {
		return nil, err
	}
	var credential any
	if body := optionalRaw(input, "credential"); string(body) != "null" {
		var draft struct {
			Kind       string  `json:"kind"`
			Secret     string  `json:"secret"`
			Passphrase *string `json:"passphrase"`
		}
		if protocol.DecodeObject(body, &draft, []string{"kind", "secret"}, true) != nil {
			return nil, hostInvalid("invalid SSH credential")
		}
		secret, err := owner.native.secret(draft.Secret)
		if err != nil {
			return nil, err
		}
		switch draft.Kind {
		case "password":
			credential = map[string]any{"kind": "password", "password": secret}
		case "private_key":
			var passphrase *protocol.Secret
			if draft.Passphrase != nil {
				value, err := owner.native.secret(*draft.Passphrase)
				if err != nil {
					return nil, err
				}
				passphrase = &value
			}
			credential = map[string]any{"kind": "private_key", "key": secret, "passphrase": passphrase}
		default:
			return nil, hostInvalid("invalid SSH credential")
		}
	}
	return owner.prepareData(ctx, "save_ssh", map[string]any{"profile": profile, "expected_revision": hostAt(profile, "revision"), "credential": credential})
}

func (owner *Host) installationModule() HostModule {
	return HostModule{Name: "sailry/installations", Exports: []HostExport{
		hostAsync("read", func(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
			ids, err := owner.ownedInstallations(arguments)
			if err != nil {
				return nil, err
			}
			return owner.readData(ctx, "read_installations", map[string]any{"requests": ids})
		}),
		hostSync("watch", owner.watchInstallations),
		hostAsync("next", func(ctx context.Context, _ []json.RawMessage) (json.RawMessage, error) {
			select {
			case owner.installationPending <- struct{}{}:
				defer func() { <-owner.installationPending }()
			default:
				return nil, hostInvalid("an installation update request is already pending")
			}
			owner.mu.Lock()
			scope := owner.installation
			owner.mu.Unlock()
			if scope == nil {
				return nil, hostInvalid("installation observation has not started")
			}
			for {
				scope.mu.Lock()
				value, failure, changed := append(json.RawMessage{}, scope.latest...), scope.err, scope.changed
				fresh := scope.first || scope.revision != scope.consumed
				if fresh {
					scope.first, scope.consumed = false, scope.revision
				}
				scope.mu.Unlock()
				if err := scope.ctx.Err(); err != nil {
					return nil, hostInvalid("installation observation changed")
				}
				if failure != nil {
					return nil, failure
				}
				if fresh {
					return value, nil
				}
				select {
				case <-ctx.Done():
					return nil, ctx.Err()
				case <-scope.ctx.Done():
					return nil, hostInvalid("installation observation changed")
				case <-changed:
				}
			}
		}),
		hostAsync("dismiss", func(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
			identity, err := hostValue[protocol.RequestID](arguments, 0)
			if err != nil {
				return nil, err
			}
			encoded, _ := hostJSON([]protocol.RequestID{identity})
			if _, err := owner.ownedInstallations([]json.RawMessage{encoded}); err != nil {
				return nil, err
			}
			if _, err := owner.readData(ctx, "dismiss_installation", map[string]any{"request": identity}); err != nil {
				return nil, err
			}
			return json.RawMessage("null"), nil
		}),
	}}
}

func (owner *Host) ownedInstallations(arguments []json.RawMessage) ([]protocol.RequestID, error) {
	identities, err := hostValue[[]protocol.RequestID](arguments, 0)
	if err != nil {
		return nil, err
	}
	if len(identities) == 0 || len(identities) > 64 {
		return nil, hostInvalid("installation request IDs must be bounded and unique")
	}
	scope := owner.scope()
	owner.mu.Lock()
	defer owner.mu.Unlock()
	unique := map[protocol.RequestID]bool{}
	for _, identity := range identities {
		if unique[identity] {
			return nil, hostInvalid("installation request IDs must be bounded and unique")
		}
		unique[identity] = true
		request, exists := owner.requests[identity.String()]
		if !exists || request.Command.Kind != "install_local_model" || !reflect.DeepEqual(request.Plugin, &scope) {
			return nil, &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "installation request is not owned by this plugin view"}
		}
	}
	return identities, nil
}

func (owner *Host) watchInstallations(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
	identities, err := owner.ownedInstallations(arguments)
	if err != nil {
		return nil, err
	}
	if owner.binding.Client == nil {
		return nil, hostUnavailable("plugin Client is unavailable")
	}
	operation, cancel := context.WithCancel(owner.ctx)
	scope := &hostInstallationScope{
		ctx: operation, cancel: cancel, changed: make(chan struct{}),
		latest: json.RawMessage("[]"), first: true,
	}
	owner.mu.Lock()
	if owner.installation != nil {
		owner.installation.cancel()
	}
	owner.installation = scope
	owner.mu.Unlock()
	go func() {
		var failure error
		defer func() {
			if failure != nil && operation.Err() == nil {
				scope.publish(nil, failure)
			}
		}()
		command, err := commandFrom("read_installations", map[string]any{"requests": identities})
		if err != nil {
			failure = err
			return
		}
		request := owner.binding.Client.Prepare(command)
		context := owner.scope()
		request.Plugin = &context
		stream, err := owner.binding.Client.Subscribe(operation, protocol.Topic{Kind: "scoped_installations", Data: request})
		if err != nil {
			failure = err
			return
		}
		defer stream.Close()
		for operation.Err() == nil {
			update, err := stream.Next(operation)
			if err != nil {
				failure = err
				return
			}
			body, _ := json.Marshal(update.Data)
			var data protocol.UpdateInstallationsPayload
			if update.Kind != "installations" || json.Unmarshal(body, &data) != nil || data.Node != owner.target() {
				failure = hostInvalid("invalid installation observation")
				return
			}
			value, err := publicHostOutput(protocol.Output{Kind: "installations", Data: data.Tasks})
			if err != nil {
				failure = err
				return
			}
			encoded, err := hostJSON(hostAt(value, "data"))
			if err != nil {
				failure = err
				return
			}
			scope.publish(encoded, nil)
		}
	}()
	return json.RawMessage("null"), nil
}

type hostInstallationScope struct {
	ctx      context.Context
	cancel   context.CancelFunc
	mu       sync.Mutex
	changed  chan struct{}
	latest   json.RawMessage
	err      error
	first    bool
	revision uint64
	consumed uint64
}

func (scope *hostInstallationScope) publish(value json.RawMessage, failure error) {
	scope.mu.Lock()
	defer scope.mu.Unlock()
	if value != nil {
		scope.latest = append(scope.latest[:0], value...)
	}
	scope.err = failure
	scope.revision++
	close(scope.changed)
	scope.changed = make(chan struct{})
}
