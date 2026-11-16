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
	// A shared host already has an administrator. The next controller's personal
	// configuration is cached in its own authenticated nonadministrator namespace.
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
			if value.ID == provider.ID || value.Source != nil && value.Source.Node == source.ID() {
				return value
			}
		}
		return protocol.ConversationProvider{}
	}
	var copied protocol.ConversationProvider
	wait("initial worker copy", func() bool {
		copied = readCopy(remote)
		return copied.Source != nil && copied.Credential != nil
	})
	catalog, err := local.ReadResourceCatalog(ctx)
	if err != nil || catalog.Entry != source.ID() || len(catalog.Sources) != 2 {
		t.Fatalf("controller resource directory is unavailable: %+v: %v", catalog, err)
	}
	receiving, err := remote.ReadConfigurationFollow(ctx)
	if err != nil {
		t.Fatal(err)
	}
	if receiving.User == nil || receiving.User.Administrator || receiving.Source == nil || *receiving.Source != source.ID() || copied.Source.Node != source.ID() || copied.Credential == nil {
		t.Fatalf("personal configuration mirror is invalid: %#v %#v", receiving, copied)
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
	if !policy.Configuration || !policy.Secrets {
		t.Fatalf("personal configuration defaults = %#v", policy)
	}
	if copied.Credential.Node != source.ID() || copied.Credential.ID != provider.Credential.ID {
		t.Fatalf("cached credential lost its controller identity: %#v", copied.Credential)
	}
	owner := store.WithUser(ctx, *receiving.User)
	checkKey := func(expected string) error {
		t.Helper()
		return target.profile.Store.Node.Read(owner, func(db *sql.Conn) error {
			key, err := store.ReadRuntimeProviderCredential(owner, db, target.ID(), copied, *copied.Credential)
			if err != nil {
				return err
			}
			if key == nil || key.Expose() != expected {
				return errors.New("cached runtime credential differs")
			}
			var materialized int
			if err := db.QueryRowContext(owner, "SELECT count(*) FROM providers WHERE id=?1 AND user=?2", copied.ID.String(), receiving.User.ID.String()).Scan(&materialized); err != nil {
				return err
			}
			if materialized != 0 {
				return errors.New("controller cache was materialized as executor configuration")
			}
			return nil
		})
	}
	waitKey := func(expected string) {
		t.Helper()
		lastFailure := ""
		wait("committed runtime credential", func() bool {
			err := checkKey(expected)
			if err == nil {
				return true
			}
			status, readErr := local.ReadConfigurationFollow(ctx)
			if readErr == nil {
				for _, target := range status.Targets {
					if target.Host == address.ID && target.Error != nil && target.Error.Error() != lastFailure {
						lastFailure = target.Error.Error()
						t.Logf("configuration delivery: %s", lastFailure)
					}
				}
			}
			return false
		})
	}
	waitKey(forwardedKey)
	provider.Name = "Updated source provider"
	updatedKey := protocol.NewSecret("configuration-follow-updated-key")
	if _, err := local.Do(ctx, protocol.Command{Kind: "save_provider", Data: protocol.CommandSaveProviderPayload{Provider: provider, ExpectedRevision: provider.Revision, Secret: &updatedKey}}); err != nil {
		t.Fatal(err)
	}
	wait("committed source change wakes worker", func() bool {
		copied = readCopy(remote)
		return copied.Name == provider.Name && copied.Source != nil && copied.Source.Revision > provider.Revision && copied.Credential != nil
	})
	waitKey(updatedKey.Expose())
	capture := copied
	local.Close()
	remote.Close()
	if err := source.Stop(context.Background()); err != nil {
		t.Fatal(err)
	}
	copied = capture
	if copied.Name != provider.Name || copied.Source == nil || copied.Source.Node != source.ID() || copied.Credential == nil {
		t.Fatal("target lost followed configuration when source stopped")
	}
	if err := checkKey(updatedKey.Expose()); err != nil {
		t.Fatal(err)
	}
	config := protocol.DefaultSessionConfig()
	config.Provider, config.Model, config.Credential = copied.ID, copied.DefaultModel, copied.Credential
	config.Mode, config.Permission = protocol.WorkModeCode, protocol.PermissionProject
	request := protocol.NewRequest(target.ID(), protocol.Command{Kind: "create_session", Data: &protocol.CommandCreateSessionPayload{Config: &config}})
	accepted, err := target.Dispatch(owner, target.ID(), request)
	if err != nil {
		t.Fatal(err)
	}
	result := <-accepted.Completion
	if result.Fault != nil {
		t.Fatal(result.Fault)
	}
	session, ok := dispatchPayload[protocol.Session](result.Output.Data)
	if !ok || session.Config.Provider != copied.ID || session.Config.Credential == nil || *session.Config.Credential != *copied.Credential {
		t.Fatal("offline source copy cannot configure a target session")
	}
}
