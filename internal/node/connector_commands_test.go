package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"runtime"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestConnectorBindChecksSelectedCLIAccount(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("shell fixture requires a Unix executable")
	}
	directory := t.TempDir()
	script := `#!/bin/sh
printf '%s' '{"hosts":{"github.com":[{"login":"current-account","active":true}]}}'
`
	if err := os.WriteFile(filepath.Join(directory, "gh"), []byte(script), 0o700); err != nil {
		t.Fatal(err)
	}
	environment := process.NewEnvironment([]string{"PATH=" + directory})
	node := systemNodeFixture(t, Options{Environment: &environment, MCP: fixtureMCPOptions()})
	ctx, cancel := context.WithTimeout(t.Context(), 10*time.Second)
	defer cancel()
	var previous protocol.PluginConnectorConnection
	if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		info, err := store.ReadPlugin(ctx, db, "github")
		if err != nil {
			return err
		}
		if info == nil {
			return errors.New("GitHub connector is absent")
		}
		input := protocol.CommandStartConnectorLoginPayload{Package: plugins.PackageReference(*info), Connector: "github", Method: protocol.PluginConnectorKindSecret}
		input.Input.Secrets = map[string]protocol.Secret{"token": protocol.NewSecret("fixture-token")}
		capture, err := store.PrepareConnectorLogin(ctx, db, protocol.NewRequestID(), input)
		if err != nil {
			return err
		}
		if err := store.StartConnectorLogin(ctx, db, capture); err != nil {
			return err
		}
		previous, err = store.CompleteConnectorLogin(ctx, db, capture, nil, nil, false)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	input := protocol.CommandStartConnectorLoginPayload{Package: previous.Package, Connector: previous.Connector, Connection: &previous.ID,
		ExpectedRevision: &previous.Revision, Method: protocol.PluginConnectorKindLocalCli, ExpectedCLIAccount: new("old-account")}
	command := protocol.Command{Kind: "start_connector_login", Data: &input}
	result := dispatchFixture(t, node, node.ID(), protocol.NewRequest(node.ID(), command))
	if result.Fault == nil || result.Fault.Code != protocol.ErrorCodeRevisionConflict {
		t.Fatalf("binding with a changed account returned %v", result.Fault)
	}
	if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		current, err := store.ReadConnectorRecord(ctx, db, previous.ID)
		if err == nil && (current.Method != previous.Method || current.Revision != previous.Revision || current.Secrets["token"].Expose() == "") {
			t.Error("rejecting the stale selection changed the saved connection")
		}
		return err
	}); err != nil {
		t.Fatal(err)
	}
	input.ExpectedCLIAccount = new("current-account")
	attempt := commandFixture(t, node, command).Data.(protocol.PluginConnectorAttempt)
	updates, err := node.Subscribe(ctx, node.ID(), protocol.Topic{Kind: "connector_login", Data: attempt.ID})
	if err != nil {
		t.Fatal(err)
	}
	defer updates.Close()
	for {
		update, err := updates.Next(ctx)
		if err != nil {
			t.Fatal(err)
		}
		login, valid := dispatchPayload[protocol.PluginConnectorLoginUpdate](update.Data)
		if !valid || login.State.Kind == "failed" || login.State.Kind == "cancelled" {
			t.Fatalf("unexpected CLI login state %q", login.State.Kind)
		}
		if login.State.Kind == "connected" {
			connection, valid := dispatchPayload[protocol.PluginConnectorConnection](login.State.Data)
			if !valid || connection.Identity == nil || connection.Identity.ID != "current-account" || len(connection.Configured) != 0 {
				t.Error("binding did not retain the selected account with its own credentials")
			}
			break
		}
	}
}

// Over 1 s: the Node start installs and verifies the bundled packages.
func TestPluginConnectorAccess(t *testing.T) {
	node := nodeFixture(t)
	ctx := t.Context()
	entry := "dev.sailry.platform/host/main.js"
	install := func(name string) (protocol.PluginInfo, protocol.PluginConnectorConnection) {
		t.Helper()
		source := t.TempDir()
		for file, content := range map[string]string{
			"plugin.json": `{"$schema":"` + plugins.ManifestSchema + `","name":"` + name + `","version":"0.1.0","extensions":{"dev.sailry.platform":{"api_version":"v1","actions":[],"connectors":[{"id":"account","display":{"label":"Fixture account"},"accounts":"single","origins":["https://fixture.test"],"auth":[{"kind":"secret","fields":["token"]}],"fields":[{"name":"token","title":"Token","secret":true}],"inject":[{"server":"server","header":"Authorization","prefix":"Bearer ","field":"token"}]}],"host":{"entry":"` + entry + `","resources":["` + entry + `"],"handlers":["probe"],"read_only":["probe"]}}}}`,
			"mcp.json":    `{"$schema":"https://agent-plugins.org/schemas/1.0.0/mcp.schema.json","mcpServers":{"server":{"type":"streamable-http","url":"https://fixture.test/mcp"}}}`,
			entry: `import {list,types,status} from "sailry/connectors";
export async function probe(){const own=await status("account");return {rows:await list(),types:await types(),state:own?.state??null};}`,
		} {
			filename := filepath.Join(source, filepath.FromSlash(file))
			if err := os.MkdirAll(filepath.Dir(filename), 0o700); err != nil {
				t.Fatal(err)
			}
			if err := os.WriteFile(filename, []byte(content), 0o600); err != nil {
				t.Fatal(err)
			}
		}
		info, err := node.plugins.manager.Install(ctx, source, "", name)
		if err != nil || len(info.Issues) != 0 {
			t.Fatalf("connector fixture package: %v, %+v", err, info.Issues)
		}
		var connection protocol.PluginConnectorConnection
		err = node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
			var err error
			if info, err = store.InstallPlugin(ctx, db, info, 0, true); err != nil {
				return err
			}
			capture, err := store.PrepareConnectorLogin(ctx, db, protocol.NewRequestID(), protocol.CommandStartConnectorLoginPayload{
				Package: plugins.PackageReference(info), Connector: "account", Method: protocol.PluginConnectorKindSecret,
				Input: protocol.PluginConnectorInput{Fields: map[string]string{}, Secrets: map[string]protocol.Secret{"token": protocol.NewSecret("token-" + name)}},
			})
			if err != nil {
				return err
			}
			if err := store.StartConnectorLogin(ctx, db, capture); err != nil {
				return err
			}
			connection, err = store.CompleteConnectorLogin(ctx, db, capture, nil, nil, false)
			return err
		})
		if err != nil {
			t.Fatal(err)
		}
		return info, connection
	}
	one, oneConnection := install("connector-one")
	two, twoConnection := install("connector-two")
	scoped := func(info protocol.PluginInfo, command protocol.Command) protocol.Request {
		scope := protocol.DefaultPluginContext()
		scope.Package = plugins.PackageReference(info)
		request := protocol.NewRequest(node.ID(), command)
		request.Plugin = &scope
		return request
	}
	owned := func(t *testing.T, output protocol.Output, name string) {
		t.Helper()
		raw, err := json.Marshal(output.Data)
		if err != nil {
			t.Fatal(err)
		}
		var rows []struct {
			Package protocol.PluginReference `json:"package"`
		}
		if err := json.Unmarshal(raw, &rows); err != nil || len(rows) != 1 || rows[0].Package.Name != name {
			t.Fatalf("%s lists %s", name, raw)
		}
	}

	t.Run("a package handler reads only its own connector state", func(t *testing.T) {
		for _, info := range []protocol.PluginInfo{one, two} {
			result := dispatchFixture(t, node, node.ID(), scoped(info, protocol.Command{Kind: "read_plugin_callback", Data: protocol.CommandReadPluginCallbackPayload{Handler: "probe", Input: json.RawMessage("null")}}))
			if result.Fault != nil {
				t.Fatalf("%s: %v", info.Summary.Name, result.Fault)
			}
			raw, _ := json.Marshal(result.Output.Data)
			var probe struct {
				Rows, Types []struct {
					Package struct {
						Name string `json:"name"`
					} `json:"package"`
				}
				State string `json:"state"`
			}
			if err := json.Unmarshal(raw, &probe); err != nil || len(probe.Rows) != 1 || len(probe.Types) != 1 ||
				probe.Rows[0].Package.Name != info.Summary.Name || probe.Types[0].Package.Name != info.Summary.Name || probe.State != "connected" {
				t.Fatalf("%s saw %s (%v)", info.Summary.Name, raw, err)
			}
		}
	})
	t.Run("scoped lists are filtered and controller lists are complete", func(t *testing.T) {
		for _, kind := range []string{"list_connections", "list_connector_types"} {
			result := dispatchFixture(t, node, node.ID(), scoped(one, protocol.Command{Kind: kind}))
			if result.Fault != nil {
				t.Fatalf("%s: %v", kind, result.Fault)
			}
			owned(t, result.Output, "connector-one")
			raw, _ := json.Marshal(commandFixture(t, node, protocol.Command{Kind: kind}).Data)
			var rows []struct {
				Package protocol.PluginReference `json:"package"`
			}
			names := map[string]bool{}
			if err := json.Unmarshal(raw, &rows); err != nil {
				t.Fatal(err)
			}
			for _, row := range rows {
				names[row.Package.Name] = true
			}
			if !names["connector-one"] || !names["connector-two"] {
				t.Fatalf("controller %s lost a package: %s", kind, raw)
			}
		}
	})
	t.Run("connection management and foreign logins stay refused", func(t *testing.T) {
		login := func(reference protocol.PluginReference, method protocol.PluginConnectorKind, connection *protocol.PluginConnectorID) protocol.Command {
			return protocol.Command{Kind: "start_connector_login", Data: &protocol.CommandStartConnectorLoginPayload{
				Package: reference, Connector: "account", Method: method, Connection: connection,
				Input: protocol.PluginConnectorInput{Fields: map[string]string{}, Secrets: map[string]protocol.Secret{}},
			}}
		}
		for name, command := range map[string]protocol.Command{
			"disable":            {Kind: "set_connection_enabled", Data: &protocol.CommandSetConnectionEnabledPayload{Connection: oneConnection.ID, ExpectedRevision: oneConnection.Revision, Enabled: false}},
			"remove":             {Kind: "remove_connection", Data: &protocol.CommandRemoveConnectionPayload{Connection: oneConnection.ID, ExpectedRevision: oneConnection.Revision}},
			"probe":              {Kind: "probe_connection", Data: &protocol.CommandProbeConnectionPayload{Connection: oneConnection.ID}},
			"foreign login":      login(plugins.PackageReference(two), protocol.PluginConnectorKindMcpOauth, nil),
			"secret login":       login(plugins.PackageReference(one), protocol.PluginConnectorKindSecret, nil),
			"foreign connection": login(plugins.PackageReference(one), protocol.PluginConnectorKindMcpOauth, &twoConnection.ID),
		} {
			if _, err := node.Dispatch(ctx, node.ID(), scoped(one, command)); !pluginMessageFault(err, protocol.ErrorCodePermissionDenied) {
				t.Errorf("%s: %v", name, err)
			}
		}
	})
	t.Run("a disabled package loses connector access", func(t *testing.T) {
		if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
			_, err := store.SetPluginEnabled(ctx, db, two.Summary.Name, two.Summary.Revision, false)
			return err
		}); err != nil {
			t.Fatal(err)
		}
		if _, err := node.Dispatch(ctx, node.ID(), scoped(two, protocol.Command{Kind: "list_connections"})); err == nil {
			t.Fatal("a disabled package listed its connections")
		}
	})
}
