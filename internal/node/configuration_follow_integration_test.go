//go:build integration

package node

import (
	"context"
	"database/sql"
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

func TestConfigurationFollowKeepsModelProvidersOnTheirNodeIntegration(t *testing.T) {
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
	var follower protocol.ConfigurationFollowStatus
	wait("paired user", func() bool {
		var err error
		follower, err = remote.ReadConfigurationFollow(ctx)
		if err != nil {
			t.Fatal(err)
		}
		return follower.User != nil && follower.Source != nil && *follower.Source == source.ID()
	})
	list := func(controller *client.Client) []protocol.ConversationProvider {
		t.Helper()
		output, err := controller.Do(ctx, protocol.Command{Kind: "list_providers"})
		if err != nil {
			t.Fatal(err)
		}
		values, ok := dispatchPayload[[]protocol.ConversationProvider](output.Data)
		if !ok {
			t.Fatal("provider listing has an invalid payload")
		}
		return values
	}
	if values := list(remote); len(values) != 0 {
		t.Fatalf("controller providers appeared on the execution Node: %+v", values)
	}
	if values := list(local); len(values) != 1 || values[0].ID != provider.ID {
		t.Fatalf("source provider changed: %+v", values)
	}
	if provider.Credential == nil {
		t.Fatal("source provider has no credential capture")
	}
	owner := store.WithUser(ctx, *follower.User)
	// Wait for the category to reconcile before checking the empty inventory.
	wait("runtime credentials reconciled", func() bool {
		var manifest protocol.ConfigurationManifest
		err := target.profile.Store.Node.Read(owner, func(db *sql.Conn) error {
			var err error
			manifest, err = store.ReadConfigurationManifest(owner, db, source.ID(), "runtime_credentials")
			return err
		})
		if err != nil {
			t.Fatal(err)
		}
		if len(manifest.Records) != 0 {
			t.Fatal("a newly paired host received a controller credential")
		}
		return manifest.Revision != 0
	})

	// The same stable ID identifies the user's selected channel, while each
	// Node independently owns its endpoint, credential, revision and default.
	localProvider := provider
	localProvider.Owner, localProvider.Source, localProvider.Consent = nil, nil, nil
	localProvider.Revision, localProvider.Shared = 0, false
	localProvider.Name = "Execution Node channel"
	localProvider.Endpoint = "https://execution.invalid/v1"
	localProvider.Credential = nil
	key := protocol.NewSecret("execution-node-key")
	output, err := remote.Do(ctx, protocol.Command{Kind: "save_provider", Data: protocol.CommandSaveProviderPayload{
		Provider: localProvider, ExpectedRevision: 0, Secret: &key,
	}})
	if err != nil {
		t.Fatal(err)
	}
	saved, ok := dispatchPayload[protocol.ConversationProvider](output.Data)
	if !ok || saved.Credential == nil || saved.Credential.Node != target.ID() || saved.ID != provider.ID || saved.Name != localProvider.Name {
		t.Fatalf("execution Node did not save its own channel: %+v", output)
	}
	secretOutput, err := remote.Do(ctx, protocol.Command{Kind: "read_provider_key", Data: protocol.CommandReadProviderKeyPayload{Provider: saved.ID, ExpectedRevision: saved.Revision}})
	if err != nil {
		t.Fatal(err)
	}
	secret, ok := dispatchPayload[*protocol.Secret](secretOutput.Data)
	if !ok || secret == nil || secret.Expose() != key.Expose() {
		t.Fatal("execution Node did not retain its own API key")
	}
	if err := target.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var localRows, copiedRows int
		if err := db.QueryRowContext(ctx, "SELECT count(*) FROM providers WHERE id=?1 AND user=?2", saved.ID.String(), follower.User.ID.String()).Scan(&localRows); err != nil {
			return err
		}
		if err := db.QueryRowContext(ctx, "SELECT count(*) FROM configuration_cache WHERE user=?1 AND category='providers' AND id=?2", follower.User.ID.String(), saved.ID.String()).Scan(&copiedRows); err != nil {
			return err
		}
		if localRows != 1 || copiedRows != 0 {
			return errors.New("provider is not isolated as the paired user's local configuration")
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	local.Close()
	remote.Close()
	if err := source.Stop(context.Background()); err != nil {
		t.Fatal(err)
	}
	if err := target.profile.Store.Node.Read(owner, func(db *sql.Conn) error {
		output, err := store.ReadProviderCommand(owner, db, target.ID(), protocol.Command{
			Kind: "read_provider_key",
			Data: protocol.CommandReadProviderKeyPayload{Provider: saved.ID, ExpectedRevision: saved.Revision},
		})
		if err != nil {
			return err
		}
		secret, ok := dispatchPayload[*protocol.Secret](output.Data)
		if !ok || secret == nil || secret.Expose() != key.Expose() {
			return errors.New("the execution Node lost its own provider key")
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}
