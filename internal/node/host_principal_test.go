package node

import (
	"database/sql"
	"testing"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestHostCommandsRetainRemotePrincipal(t *testing.T) {
	t.Parallel()
	node := moduleFixtureNode(t, "ssh")
	ctx := t.Context()
	owner, other := protocol.NodeID{94}, protocol.NodeID{95}
	if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		if err := store.PairLinkPeer(ctx, db, owner, link.DirectionController); err != nil {
			return err
		}
		return store.PairLinkPeer(ctx, db, other, link.DirectionController)
	}); err != nil {
		t.Fatal(err)
	}
	send := func(caller protocol.NodeID, command protocol.Command) link.Response {
		t.Helper()
		return dispatchFixture(t, node, caller, protocol.NewRequest(node.ID(), command))
	}
	profile := protocol.SshProfile{ID: protocol.NewSshID(), Name: "Personal host", Host: "private.fixture.test", Port: 22, Username: "fixture", Authentication: protocol.SshAuthenticationPassword}
	credential := protocol.SshCredential{Kind: "password", Data: &protocol.SshCredentialPasswordPayload{Password: protocol.NewSecret("owner-fixture-password")}}
	saved := send(owner, protocol.Command{Kind: "save_ssh", Data: protocol.CommandSaveSshPayload{Profile: profile, Credential: &credential}})
	if saved.Fault != nil {
		t.Fatal(saved.Fault)
	}
	var ok bool
	profile, ok = dispatchPayload[protocol.SshProfile](saved.Output.Data)
	if !ok {
		t.Fatalf("SSH save output: %#v", saved.Output)
	}
	for _, caller := range []protocol.NodeID{owner, other, node.ID()} {
		listed := send(caller, protocol.Command{Kind: "list_ssh"})
		values, valid := dispatchPayload[[]protocol.SshProfile](listed.Output.Data)
		want := 0
		if caller == owner {
			want = 1
		}
		if listed.Fault != nil || !valid || len(values) != want || want == 1 && values[0].ID != profile.ID {
			t.Fatalf("SSH namespace for %s: %#v", caller, listed)
		}
	}
	profile.Name = "Renamed personal host"
	command := protocol.Command{Kind: "save_ssh", Data: protocol.CommandSaveSshPayload{Profile: profile, ExpectedRevision: profile.Revision}}
	if denied := send(other, command); denied.Fault == nil {
		t.Fatal("another user changed a private SSH profile")
	}
	updated := send(owner, command)
	if updated.Fault != nil {
		t.Fatal(updated.Fault)
	}
	profile, ok = dispatchPayload[protocol.SshProfile](updated.Output.Data)
	if !ok {
		t.Fatal("updated SSH profile missing")
	}
	if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		user, err := store.CallerUser(ctx, db, node.ID(), owner)
		if err != nil {
			return err
		}
		_, key, err := store.ReadSsh(store.WithUser(ctx, user), db, profile.ID, profile.Revision)
		if err == nil {
			password, valid := dispatchPayload[protocol.SshCredentialPasswordPayload](key.Data)
			if !valid || password.Password.Expose() != "owner-fixture-password" {
				t.Fatal("SSH update lost the admitted user's credential")
			}
		}
		return err
	}); err != nil {
		t.Fatal(err)
	}
}
