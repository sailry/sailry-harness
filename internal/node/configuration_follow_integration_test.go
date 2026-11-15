//go:build integration

package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"path/filepath"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Real loopback transports exercise the follow workers and target observers. No
// model request is made, and MCP startup uses the deterministic unavailable fixture.
func TestConfigurationFollowIntegration(t *testing.T) {
	ctx, cancel := context.WithTimeout(t.Context(), 60*time.Second)
	defer cancel()
	scope := link.NetworkScope{Bind: "127.0.0.1:0"}
	environment := process.NewEnvironment([]string{"PATH=/usr/bin:/bin"})
	start := func(name string) *Node {
		node, err := StartWithOptions(ctx, filepath.Join(t.TempDir(), name), Options{Network: &scope, Environment: &environment, MCP: fixtureMCPOptions()})
		if err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() {
			if err := node.Stop(context.Background()); err != nil {
				t.Error(err)
			}
		})
		return node
	}
	source, target := start("source"), start("target")
	if err := source.Link().SetName("Source workstation"); err != nil {
		t.Fatal(err)
	}
	if err := target.Link().SetName("Target server"); err != nil {
		t.Fatal(err)
	}
	// A public host already has an administrator. Its next paired user must not
	// receive the source's static credentials until that host is opted in.
	if err := target.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		_, err := store.CreateUser(ctx, db, "Existing administrator")
		return err
	}); err != nil {
		t.Fatal(err)
	}
	provider := forwardingProvider(t, source, "https://configuration.invalid/v1")
	invitation, err := target.Link().Invite(ctx)
	if err != nil {
		t.Fatal(err)
	}
	defer invitation.Close()
	address, err := source.Link().Pair(ctx, invitation.Ticket())
	if err != nil {
		t.Fatal(err)
	}
	if err := invitation.Paired(ctx); err != nil {
		t.Fatal(err)
	}
	connect := func(transport link.Transport) *client.Client {
		controller := client.New(transport)
		t.Cleanup(func() { _ = controller.Close() })
		if err := controller.Start(ctx); err != nil {
			t.Fatal(err)
		}
		return controller
	}
	local := connect(link.NewLocal(source.ID(), source.ID(), source))
	remote := connect(source.Link().Remote(address))
	wait := func(label string, ready func() bool) {
		t.Helper()
		ticker := time.NewTicker(25 * time.Millisecond)
		defer ticker.Stop()
		for !ready() {
			select {
			case <-ctx.Done():
				t.Fatalf("%s: %v", label, context.Cause(ctx))
			case <-ticker.C:
			}
		}
	}
	readCopy := func(controller *client.Client) protocol.ConversationProvider {
		t.Helper()
		output, err := controller.Do(ctx, protocol.Command{Kind: "list_providers"})
		if err != nil {
			t.Fatal(err)
		}
		values, ok := dispatchPayload[[]protocol.ConversationProvider](output.Data)
		if !ok {
			t.Fatal("provider list has an invalid payload")
		}
		for _, value := range values {
			if value.ID == provider.ID {
				return value
			}
		}
		return protocol.ConversationProvider{}
	}
	var copied protocol.ConversationProvider
	wait("initial worker copy", func() bool {
		copied = readCopy(remote)
		return copied.Source != nil
	})
	if err := source.SyncIdentity(ctx); err != nil {
		t.Fatal(err)
	}
	directoryOutput, err := local.Do(ctx, protocol.Command{Kind: "identity", Data: protocol.IdentityCommand{Kind: "read_host_directory"}})
	if err != nil {
		t.Fatal(err)
	}
	directory, ok := dispatchPayload[protocol.HostDirectory](directoryOutput.Data)
	if !ok {
		t.Fatal("host directory has an invalid payload")
	}
	names := map[protocol.NodeID]string{}
	for _, host := range directory.Hosts {
		names[host.Node] = host.Name
	}
	if names[source.ID()] != "Source workstation" || names[target.ID()] != "Target server" {
		t.Fatalf("host directory lost authenticated display names: %#v", names)
	}
	receiving, err := remote.ReadConfigurationFollow(ctx)
	if err != nil {
		t.Fatal(err)
	}
	if receiving.User == nil || receiving.User.Administrator || receiving.Source == nil || *receiving.Source != source.ID() || copied.Source.Node != source.ID() || copied.Credential != nil {
		t.Fatalf("public host initial follow policy or copy is invalid: %#v %#v", receiving, copied)
	}
	denied := func(command protocol.Command) {
		t.Helper()
		_, err := remote.Do(ctx, command)
		var fault *link.FaultError
		if !errors.As(err, &fault) || fault.Fault.Code != protocol.ErrorCodePermissionDenied {
			t.Fatalf("%s on a followed provider: %v", command.Kind, err)
		}
	}
	denied(protocol.Command{Kind: "put_provider", Data: protocol.CommandPutProviderPayload{Provider: copied, ExpectedRevision: copied.Revision}})
	denied(protocol.Command{Kind: "read_provider_key", Data: protocol.CommandReadProviderKeyPayload{Provider: copied.ID, ExpectedRevision: copied.Revision}})
	var policy protocol.ConfigurationFollowTarget
	wait("source discovers target policy", func() bool {
		status, err := local.ReadConfigurationFollow(ctx)
		if err != nil {
			t.Fatal(err)
		}
		for _, value := range status.Targets {
			if value.Host == target.ID() {
				policy = value
				return true
			}
		}
		return false
	})
	if !policy.Configuration || policy.Secrets {
		t.Fatalf("public host target defaults = %#v", policy)
	}
	policy.Secrets = true
	if _, err := local.ConfigurationFollow(ctx, protocol.ConfigurationFollowCommand{Action: "set_target", Target: &policy, ExpectedRevision: policy.Revision}); err != nil {
		t.Fatal(err)
	}
	wait("static credential opt-in", func() bool {
		copied = readCopy(remote)
		return copied.Credential != nil
	})
	if copied.Credential.Node != target.ID() || copied.Credential.ID == provider.Credential.ID {
		t.Fatalf("copied credential was not scoped to target: %#v", copied.Credential)
	}
	assertKey := func(expected string) {
		t.Helper()
		var body []byte
		if err := target.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			return db.QueryRowContext(ctx, "SELECT body FROM providers WHERE id=?1 AND user=?2", provider.ID.String(), receiving.User.ID.String()).Scan(&body)
		}); err != nil {
			t.Fatal(err)
		}
		var row struct {
			Authorizations map[protocol.CredentialID]struct {
				Key *protocol.Secret `json:"key"`
			} `json:"authorizations"`
		}
		if err := json.Unmarshal(body, &row); err != nil {
			t.Fatal(err)
		}
		authorization := row.Authorizations[copied.Credential.ID]
		if authorization.Key == nil || authorization.Key.Expose() != expected {
			t.Fatal("target did not retain the expected static credential")
		}
	}
	assertKey(forwardedKey)
	provider.Name = "Updated source provider"
	updatedKey := protocol.NewSecret("configuration-follow-updated-key")
	if _, err := local.Do(ctx, protocol.Command{Kind: "save_provider", Data: protocol.CommandSaveProviderPayload{Provider: provider, ExpectedRevision: provider.Revision, Secret: &updatedKey}}); err != nil {
		t.Fatal(err)
	}
	wait("committed source change wakes worker", func() bool {
		copied = readCopy(remote)
		return copied.Name == provider.Name && copied.Source != nil && copied.Source.Revision > provider.Revision && copied.Credential != nil
	})
	assertKey(updatedKey.Expose())
	receiving, err = remote.ReadConfigurationFollow(ctx)
	if err != nil {
		t.Fatal(err)
	}
	paused, err := remote.ConfigurationFollow(ctx, protocol.ConfigurationFollowCommand{Action: "set_source", ExpectedRevision: receiving.Revision})
	if err != nil {
		t.Fatal(err)
	}
	provider = readCopy(local)
	provider.Name = "Source changed while following was paused"
	if _, err := local.Do(ctx, protocol.Command{Kind: "put_provider", Data: protocol.CommandPutProviderPayload{Provider: provider, ExpectedRevision: provider.Revision}}); err != nil {
		t.Fatal(err)
	}
	wait("paused target rejects delivery", func() bool {
		status, err := local.ReadConfigurationFollow(ctx)
		if err != nil {
			t.Fatal(err)
		}
		for _, value := range status.Targets {
			if value.Host == target.ID() && value.Error != nil && value.Error.Code == protocol.ErrorCodePermissionDenied {
				return true
			}
		}
		return false
	})
	if value := readCopy(remote); value.Name != copied.Name || value.Revision != copied.Revision {
		t.Fatal("disabled following changed the retained copy")
	}
	if _, err := remote.ConfigurationFollow(ctx, protocol.ConfigurationFollowCommand{Action: "set_source", Source: pointerNode(source.ID()), ExpectedRevision: paused.Revision}); err != nil {
		t.Fatal(err)
	}
	// No source edit or explicit sync follows the target policy change.
	wait("target observer resumes same-source delivery", func() bool {
		copied = readCopy(remote)
		return copied.Name == provider.Name && copied.Credential != nil
	})
	assertKey(updatedKey.Expose())

	// A second real controller belongs to the same receiving user. It remains
	// connected when the source Node stops, proving the target owns its copy.
	endpoint, err := link.OpenController(ctx, filepath.Join(t.TempDir(), "controller"), scope)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = endpoint.Close() })
	device, err := target.Link().InviteFor(ctx, link.InvitationScope{Kind: protocol.InvitationMyDevice, User: &receiving.User.ID})
	if err != nil {
		t.Fatal(err)
	}
	defer device.Close()
	controllerAddress, err := endpoint.Pair(ctx, device.Ticket())
	if err != nil {
		t.Fatal(err)
	}
	if err := device.Paired(ctx); err != nil {
		t.Fatal(err)
	}
	independent := connect(endpoint.Remote(controllerAddress))
	local.Close()
	remote.Close()
	if err := source.Stop(context.Background()); err != nil {
		t.Fatal(err)
	}
	copied = readCopy(independent)
	if copied.Name != provider.Name || copied.Source == nil || copied.Source.Node != source.ID() || copied.Credential == nil {
		t.Fatal("target lost followed configuration when source stopped")
	}
	assertKey(updatedKey.Expose())
	config := protocol.DefaultSessionConfig()
	config.Provider, config.Model, config.Credential = copied.ID, copied.DefaultModel, copied.Credential
	config.Mode, config.Permission = protocol.WorkModeCode, protocol.PermissionProject
	output, err := independent.Do(ctx, protocol.Command{Kind: "create_session", Data: protocol.CommandCreateSessionPayload{Config: &config}})
	if err != nil {
		t.Fatal(err)
	}
	session, ok := dispatchPayload[protocol.Session](output.Data)
	if !ok || session.Config.Provider != copied.ID || session.Config.Credential == nil || *session.Config.Credential != *copied.Credential {
		t.Fatal("offline source copy cannot configure a target session")
	}
}
