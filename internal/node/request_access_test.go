package node

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"testing"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/admission"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestOutcomeRecoveryUsesCurrentAccessAndSecretFreeScope(t *testing.T) {
	node := nodeFixture(t)
	ctx := t.Context()
	caller := protocol.NodeID{91}
	var user protocol.User
	if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		if err := store.PairLinkPeer(ctx, db, caller, link.DirectionController); err != nil {
			return err
		}
		var err error
		user, err = store.CallerUser(ctx, db, node.ID(), caller)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	owned := store.WithUser(ctx, user)
	secret := "scope-secret-canary"
	request := protocol.NewRequest(node.ID(), secretCommands(secret)["save_provider"])
	encoded, err := json.Marshal(request)
	if err != nil {
		t.Fatal(err)
	}
	result := link.Response{Output: protocol.Output{Kind: "power_settings", Data: protocol.PowerSettings{}}}
	if err := node.profile.Store.Node.Write(owned, func(db *sql.Conn) error {
		if _, err := admission.Admit(owned, db, caller, request, encoded); err != nil {
			return err
		}
		return admission.Complete(owned, db, caller, request.ID, result)
	}); err != nil {
		t.Fatal(err)
	}
	var target protocol.User
	if err := node.profile.Store.Node.Write(owned, func(db *sql.Conn) error {
		var body, scope []byte
		if err := db.QueryRowContext(ctx, "SELECT body,scope FROM requests WHERE caller=?1 AND id=?2", caller[:], request.ID.String()).Scan(&body, &scope); err != nil {
			return err
		}
		if bytes.Contains(body, []byte(secret)) || bytes.Contains(scope, []byte(secret)) {
			return fmt.Errorf("ledger retained authored configuration")
		}
		var captured admission.Scope
		if err := json.Unmarshal(scope, &captured); err != nil {
			return err
		}
		if captured.User == nil || *captured.User != user.ID || captured.Kind != "save_provider" {
			return fmt.Errorf("missing recovery principal: %+v", captured)
		}
		original, err := admission.Inspect(owned, db, caller, request.ID, admission.Digest(encoded))
		if err != nil {
			return err
		}
		check := func(ctx context.Context, allowed bool) error {
			value, err := node.inspectOutcomeAccess(ctx, db, caller, request.ID, original)
			if err != nil {
				return err
			}
			body, _ := json.Marshal(value)
			var decoded protocol.RequestOutcome
			if err := json.Unmarshal(body, &decoded); err != nil {
				return err
			}
			outcome := decoded.Data.(*protocol.Result[protocol.Output, protocol.Fault])
			if decoded.Kind != "completed" || (outcome.Err == nil) != allowed {
				return fmt.Errorf("recovery allowed=%v: %s", allowed, body)
			}
			if !allowed && outcome.Err.Code != protocol.ErrorCodePermissionDenied {
				return outcome.Err
			}
			return nil
		}
		if err := check(owned, true); err != nil {
			return err
		}
		target, err = store.CreateUser(ctx, db, "Surviving user")
		if err != nil {
			return err
		}
		return check(store.WithUser(ctx, target), false)
	}); err != nil {
		t.Fatal(err)
	}
}

func TestCapturedCallbackSurvivesDeviceRevocation(t *testing.T) {
	node := nodeFixture(t)
	ctx := t.Context()
	caller := protocol.NodeID{92}
	var user protocol.User
	if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		if err := store.PairLinkPeer(ctx, db, caller, link.DirectionController); err != nil {
			return err
		}
		var err error
		user, err = store.CallerUser(ctx, db, node.ID(), caller)
		if err != nil {
			return err
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	config := sessionConfigFixtureFor(t, node, caller)
	if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		return store.SetLinkTrust(ctx, db, caller, false)
	}); err != nil {
		t.Fatal(err)
	}
	owned := store.WithCapturedUser(store.WithUser(ctx, user), user.ID)
	request := protocol.NewRequest(node.ID(), protocol.Command{Kind: "create_session", Data: protocol.CommandCreateSessionPayload{Config: &config}})
	if _, err := node.Dispatch(owned, caller, request); err == nil {
		t.Fatal("public dispatch accepted a revoked caller carrying an old context")
	}
	accepted, err := node.dispatchCaptured(owned, caller, request)
	if err != nil {
		t.Fatal(err)
	}
	result := <-accepted.Completion
	if result.Fault != nil {
		t.Fatal(result.Fault)
	}
	session := result.Output.Data.(protocol.Session)
	if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var owner string
		if err := db.QueryRowContext(ctx, "SELECT owner FROM sessions WHERE id=?1", session.ID.String()).Scan(&owner); err != nil {
			return err
		}
		if owner != user.ID.String() {
			return fmt.Errorf("callback became the local user's session")
		}
		var count int
		if err := db.QueryRowContext(ctx, "SELECT count(*) FROM requests WHERE caller=?1 AND id=?2", caller[:], request.ID.String()).Scan(&count); err != nil {
			return err
		}
		if count != 1 {
			return fmt.Errorf("callback lost its original caller ledger identity")
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}

func TestTransferRetainsDeletedWorktreeScope(t *testing.T) {
	node := nodeFixture(t)
	draft := protocol.DefaultProjectsDraft()
	draft.Name, draft.Path, draft.Source = "Stream project", t.TempDir(), protocol.ProjectsSource{Kind: "local"}
	commandFixture(t, node, protocol.Command{Kind: "create_project", Data: draft})
	worktree := snapshotFixture(t, node).Worktrees[0]
	if err := os.WriteFile(filepath.Join(worktree.Path, "data.txt"), []byte("private data"), 0600); err != nil {
		t.Fatal(err)
	}
	download := commandFixture(t, node, protocol.Command{Kind: "download_file", Data: protocol.CommandDownloadFilePayload{Worktree: worktree.ID, Path: "data.txt"}}).Data.(protocol.FileDownload)
	stream, err := node.Open(t.Context(), node.ID(), download.Stream)
	if err != nil {
		t.Fatal(err)
	}
	defer stream.Close()
	if err := node.profile.Store.Node.Write(t.Context(), func(db *sql.Conn) error {
		_, err := db.ExecContext(t.Context(), "DELETE FROM worktrees WHERE id=?1", worktree.ID.String())
		return err
	}); err != nil {
		t.Fatal(err)
	}
	if _, err := stream.Read(make([]byte, 1)); err == nil {
		t.Fatal("deleted workspace transfer became an unscoped device stream")
	}
}
