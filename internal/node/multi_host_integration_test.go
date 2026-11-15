//go:build integration

package node

import (
	"context"
	"database/sql"
	"fmt"
	"net"
	"path/filepath"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestMultiHostIdentityAndOfflineRevocationIntegration(t *testing.T) {
	ctx, cancel := context.WithTimeout(t.Context(), 90*time.Second)
	defer cancel()
	pathA, pathB := filepath.Join(t.TempDir(), "source"), filepath.Join(t.TempDir(), "host")
	listener, err := net.ListenPacket("udp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	bindB := listener.LocalAddr().String()
	if err = listener.Close(); err != nil {
		t.Fatal(err)
	}
	start := func(path, bind string) *Node {
		scope := link.NetworkScope{Bind: bind}
		node, err := StartWithOptions(ctx, path, Options{Network: &scope, MCP: fixtureMCPOptions()})
		if err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() { _ = node.Stop(context.Background()) })
		return node
	}
	a, b := start(pathA, "127.0.0.1:0"), start(pathB, bindB)
	phone, err := link.OpenController(ctx, t.TempDir(), link.NetworkScope{Bind: "127.0.0.1:0"})
	if err != nil {
		t.Fatal(err)
	}
	defer phone.Close()
	pair := func(inviter *Node, claimant *link.Link, kind protocol.InvitationKind) link.EndpointAddress {
		offer, err := inviter.Link().InviteFor(ctx, link.InvitationScope{Kind: kind})
		if err != nil {
			t.Fatal(err)
		}
		defer offer.Close()
		address, err := claimant.Pair(ctx, offer.Ticket())
		if err != nil {
			t.Fatal(err)
		}
		if err = offer.Paired(ctx); err != nil {
			t.Fatal(err)
		}
		return address
	}
	addressB := pair(b, a.Link(), protocol.InvitationOtherUser)
	pair(b, phone, protocol.InvitationOtherUser)
	remoteB := client.New(phone.Remote(addressB))
	defer remoteB.Close()
	project := func(remote *client.Client, name string) protocol.Project {
		draft := protocol.DefaultProjectsDraft()
		draft.Name, draft.Path, draft.Source = name, t.TempDir(), protocol.ProjectsSource{Kind: "local"}
		output, err := remote.Do(ctx, protocol.Command{Kind: "create_project", Data: draft})
		if err != nil {
			t.Fatal(err)
		}
		value, ok := dispatchPayload[protocol.Project](output.Data)
		if !ok {
			t.Fatal("project output is invalid")
		}
		return value
	}
	phoneProject := project(remoteB, "Phone private")
	computerB := client.New(a.Link().Remote(addressB))
	defer computerB.Close()
	computerProject := project(computerB, "Computer private")
	addressA := pair(a, phone, protocol.InvitationMyDevice)
	remoteA := client.New(phone.Remote(addressA))
	defer remoteA.Close()
	phoneAddress, err := phone.Address(ctx)
	if err != nil {
		t.Fatal(err)
	}
	if err = a.SyncIdentity(ctx); err != nil {
		t.Fatal(err)
	}
	if err = client.SyncIdentity(ctx, phone, remoteA); err != nil {
		t.Fatal(err)
	}
	if err = client.SyncIdentity(ctx, phone, remoteB); err != nil {
		t.Fatal(err)
	}
	output, err := remoteB.Do(ctx, protocol.Command{Kind: "snapshot"})
	if err != nil {
		t.Fatal(err)
	}
	snapshot, ok := dispatchPayload[protocol.Snapshot](output.Data)
	if !ok || len(snapshot.Projects) != 2 {
		t.Fatalf("whole-group merge lost resources: %+v", output)
	}
	seen := map[protocol.ProjectID]bool{}
	for _, project := range snapshot.Projects {
		seen[project.ID] = true
	}
	if !seen[phoneProject.ID] || !seen[computerProject.ID] {
		t.Fatal("merged projects changed identity")
	}
	peers, err := phone.Peers(ctx)
	if err != nil || len(peers) != 2 {
		t.Fatalf("direct and imported host duplicated: %v: %v", peers, err)
	}
	if err = remoteB.Close(); err != nil {
		t.Fatal(err)
	}
	if err = computerB.Close(); err != nil {
		t.Fatal(err)
	}
	if err = b.Stop(ctx); err != nil {
		t.Fatal(err)
	}
	commandFixture(t, a, protocol.Command{Kind: "identity", Data: protocol.IdentityCommand{Kind: "revoke_device", Data: protocol.RevokeDevice{Device: phoneAddress.ID, ConfirmLast: true}}})
	if _, err = remoteA.Do(ctx, protocol.Command{Kind: "snapshot"}); err == nil {
		t.Fatal("source kept revoked device access")
	}
	var original protocol.RequestID
	if err = a.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		deliveries, err := store.IdentityDeliveries(ctx, db)
		if err != nil {
			return err
		}
		for _, delivery := range deliveries {
			command, _ := dispatchPayload[protocol.IdentityCommand](delivery.Request.Command.Data)
			if delivery.Address.ID == addressB.ID && command.Kind == "revoke_device" {
				original = delivery.Request.ID
			}
		}
		if original == (protocol.RequestID{}) {
			return fmt.Errorf("offline revocation was not queued")
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	b = start(pathB, bindB)
	if err = a.SyncIdentity(ctx); err != nil {
		t.Fatal(err)
	}
	if err = a.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var state string
		if err := db.QueryRowContext(ctx, "SELECT state FROM identity_deliveries WHERE request=?1", original.String()).Scan(&state); err != nil {
			return err
		}
		if state != "completed" {
			return fmt.Errorf("original revocation state=%s", state)
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	if err = b.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		if _, err := store.CallerUser(ctx, db, b.ID(), phoneAddress.ID); err == nil {
			return fmt.Errorf("reconnected host retained revoked membership")
		}
		var count int
		if err := db.QueryRowContext(ctx, "SELECT count(*) FROM projects WHERE id IN (?1,?2)", phoneProject.ID.String(), computerProject.ID.String()).Scan(&count); err != nil {
			return err
		}
		if count != 2 {
			return fmt.Errorf("revocation removed user data")
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}
