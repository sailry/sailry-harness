package node

import (
	"database/sql"
	"errors"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/admission"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestPrunedSessionRequestCannotBeReadmitted(t *testing.T) {
	t.Parallel()
	ctx := t.Context()
	node := nodeFixture(t)
	caller := node.ID()
	controller := client.New(link.NewLocal(caller, caller, node))
	t.Cleanup(func() { _ = controller.Close() })
	config := sessionConfigFixture(t, node)
	request := controller.Prepare(protocol.Command{Kind: "create_session", Data: &protocol.CommandCreateSessionPayload{Config: &config}})
	output, err := controller.Settle(ctx, request)
	created, ok := dispatchPayload[protocol.Session](output.Data)
	if err != nil || output.Kind != "session" || !ok {
		t.Fatalf("first creation = %#v, %v", output, err)
	}

	now := time.Now()
	var removed int64
	err = node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		old := now.Add(-admission.Retention - time.Hour).Unix()
		if _, err := db.ExecContext(ctx, "UPDATE requests SET at=?3 WHERE caller=?1 AND id=?2", caller[:], request.ID.String(), old); err != nil {
			return err
		}
		var err error
		removed, err = admission.Prune(ctx, db, now)
		return err
	})
	if err != nil || removed != 1 {
		t.Fatalf("pruning the original receipt removed %d rows: %v", removed, err)
	}

	_, err = controller.Settle(ctx, request)
	var failure *link.FaultError
	if !errors.As(err, &failure) || failure.Fault.Code != protocol.ErrorCodeOutcomeUnknown || !client.Released(err) {
		t.Fatalf("replaying the pruned request = %v", err)
	}
	var remaining int
	err = node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		return db.QueryRowContext(ctx, "SELECT count(*) FROM requests WHERE caller=?1 AND id=?2", caller[:], request.ID.String()).Scan(&remaining)
	})
	if err != nil || remaining != 0 {
		t.Fatalf("pruned request has %d ledger rows after retry: %v", remaining, err)
	}
	if sessions := snapshotFixture(t, node).Sessions; len(sessions) != 1 || sessions[0].ID != created.ID {
		t.Fatalf("retry of pruned creation changed the session set: %#v", sessions)
	}
}
