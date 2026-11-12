package admission_test

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/admission"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestLedger(t *testing.T) {
	ctx := context.Background()
	node := protocol.NodeID{1}
	caller := protocol.NodeID{2}
	storage, err := store.Open(ctx, t.TempDir(), node)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = storage.Close() })
	write := storage.Node.Write
	request := func() (protocol.Request, []byte) {
		value := protocol.NewRequest(node, protocol.Command{Kind: "set_queue_paused", Data: &protocol.CommandSetQueuePausedPayload{}})
		body, err := json.Marshal(value)
		if err != nil {
			t.Fatal(err)
		}
		return value, body
	}
	admit := func(value protocol.Request, body []byte) *link.Response {
		t.Helper()
		var previous *link.Response
		if err := write(ctx, func(db *sql.Conn) error {
			var err error
			previous, err = admission.Admit(ctx, db, caller, value, body)
			return err
		}); err != nil {
			t.Fatal(err)
		}
		return previous
	}
	inspect := func(value protocol.Request, body []byte) protocol.RequestOutcome {
		t.Helper()
		var outcome protocol.RequestOutcome
		if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
			var err error
			outcome, err = admission.Inspect(ctx, db, caller, value.ID, admission.Digest(body))
			return err
		}); err != nil {
			t.Fatal(err)
		}
		return outcome
	}
	recover := func() {
		t.Helper()
		if err := write(ctx, func(db *sql.Conn) error { return admission.Recover(ctx, db) }); err != nil {
			t.Fatal(err)
		}
	}
	applied := link.Response{Output: protocol.Output{Kind: "notifications_updated"}}
	failed := link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeBusy, Message: "storage transaction was rolled back; retry the operation"}}

	t.Run("lost reply and duplicate retry", func(t *testing.T) {
		value, body := request()
		if outcome := inspect(value, body); outcome.Kind != "not_admitted" {
			t.Fatalf("before admission = %#v", outcome)
		}
		if previous := admit(value, body); previous != nil {
			t.Fatalf("first admission = %#v", previous)
		}
		// The duplicate of a running request is never run again.
		if previous := admit(value, body); previous == nil || previous.Fault == nil || previous.Fault.Code != protocol.ErrorCodeOutcomeUnknown {
			t.Fatalf("duplicate while running = %#v", previous)
		}
		if outcome := inspect(value, body); outcome.Kind != "admitted" {
			t.Fatalf("running = %#v", outcome)
		}
		if err := admission.Settle(ctx, write, caller, value.ID, applied); err != nil {
			t.Fatal(err)
		}
		// The reply was lost: both the retry and the query return the stored result.
		if previous := admit(value, body); previous == nil || previous.Fault != nil || previous.Output.Kind != "notifications_updated" {
			t.Fatalf("duplicate after completion = %#v", previous)
		}
		outcome := inspect(value, body)
		result, ok := outcome.Data.(protocol.Result[protocol.Output, protocol.Fault])
		if outcome.Kind != "completed" || !ok || result.Ok == nil || result.Ok.Kind != "notifications_updated" {
			t.Fatalf("completed = %#v", outcome)
		}
		changed := value
		changed.Command = protocol.Command{Kind: "set_queue_paused", Data: &protocol.CommandSetQueuePausedPayload{Paused: true}}
		other, err := json.Marshal(changed)
		if err != nil {
			t.Fatal(err)
		}
		var conflict *protocol.Fault
		err = write(ctx, func(db *sql.Conn) error {
			_, err := admission.Admit(ctx, db, caller, changed, other)
			return err
		})
		if !errors.As(err, &conflict) || conflict.Code != protocol.ErrorCodeConflict {
			t.Fatalf("same identifier with other content = %v", err)
		}
		if err := admission.Settle(ctx, write, caller, value.ID, failed); !errors.Is(err, admission.ErrNotAdmitted) {
			t.Fatalf("second result for a completed request = %v", err)
		}
	})

	t.Run("crash between admission and result", func(t *testing.T) {
		value, body := request()
		admit(value, body)
		recover() // the next start of the Node
		for range 2 {
			if outcome := inspect(value, body); outcome.Kind != "unknown" {
				t.Fatalf("interrupted = %#v", outcome)
			}
			if previous := admit(value, body); previous == nil || previous.Fault == nil || previous.Fault.Code != protocol.ErrorCodeOutcomeUnknown {
				t.Fatalf("retry of an interrupted request = %#v", previous)
			}
			// Unknown is final: a late result and another start leave it alone.
			if err := admission.Settle(ctx, write, caller, value.ID, applied); !errors.Is(err, admission.ErrNotAdmitted) {
				t.Fatalf("late result = %v", err)
			}
			recover()
		}
	})

	t.Run("an unknown outcome resolved by the Node that holds the request", func(t *testing.T) {
		unknown := link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "remote outcome could not be confirmed"}}
		for name, lose := range map[string]func(protocol.Request){
			"recorded": func(value protocol.Request) {
				if err := admission.Settle(ctx, write, caller, value.ID, unknown); err != nil {
					t.Fatal(err)
				}
			},
			"interrupted": func(protocol.Request) { recover() },
		} {
			value, body := request()
			admit(value, body)
			// A running request is settled, never resolved.
			if err := admission.Resolve(ctx, write, caller, value.ID, applied); !errors.Is(err, admission.ErrNotAdmitted) {
				t.Fatalf("%s: resolving a running request = %v", name, err)
			}
			lose(value)
			if previous := admit(value, body); previous == nil || previous.Fault == nil || previous.Fault.Code != protocol.ErrorCodeOutcomeUnknown {
				t.Fatalf("%s: before the answer = %#v", name, previous)
			}
			if err := admission.Resolve(ctx, write, caller, value.ID, applied); err != nil {
				t.Fatalf("%s: %v", name, err)
			}
			outcome := inspect(value, body)
			result, ok := outcome.Data.(protocol.Result[protocol.Output, protocol.Fault])
			if outcome.Kind != "completed" || !ok || result.Ok == nil || result.Ok.Kind != "notifications_updated" {
				t.Fatalf("%s: resolved = %#v", name, outcome)
			}
			if previous := admit(value, body); previous == nil || previous.Fault != nil || previous.Output.Kind != "notifications_updated" {
				t.Fatalf("%s: replay after the answer = %#v", name, previous)
			}
			// A known result is final: a second answer does not replace it.
			if err := admission.Resolve(ctx, write, caller, value.ID, failed); !errors.Is(err, admission.ErrNotAdmitted) {
				t.Fatalf("%s: second answer = %v", name, err)
			}
			if previous := admit(value, body); previous == nil || previous.Fault != nil {
				t.Fatalf("%s: a known result was replaced: %#v", name, previous)
			}
		}
		// A stored failure of any other kind is known and stays.
		value, body := request()
		admit(value, body)
		if err := admission.Settle(ctx, write, caller, value.ID, failed); err != nil {
			t.Fatal(err)
		}
		if err := admission.Resolve(ctx, write, caller, value.ID, applied); !errors.Is(err, admission.ErrNotAdmitted) {
			t.Fatalf("resolving a known failure = %v", err)
		}
	})

	t.Run("failed result write", func(t *testing.T) {
		defer admission.SetSettleRetry(time.Millisecond)()
		value, body := request()
		admit(value, body)
		attempts := 0
		flaky := func(ctx context.Context, callback func(*sql.Conn) error) error {
			if attempts++; attempts < 3 {
				return failed.Fault
			}
			return write(ctx, callback)
		}
		if err := admission.Settle(ctx, flaky, caller, value.ID, failed); err != nil || attempts != 3 {
			t.Fatalf("settle = %v after %d attempts", err, attempts)
		}
		outcome := inspect(value, body)
		result, ok := outcome.Data.(protocol.Result[protocol.Output, protocol.Fault])
		if outcome.Kind != "completed" || !ok || result.Err == nil || result.Err.Code != protocol.ErrorCodeBusy {
			t.Fatalf("failed request = %#v", outcome)
		}

		// A stopping Node writes once more and then leaves the row to Recover.
		value, body = request()
		admit(value, body)
		stopping, cancel := context.WithCancel(ctx)
		cancel()
		attempts = 0
		broken := func(context.Context, func(*sql.Conn) error) error { attempts++; return failed.Fault }
		if err := admission.Settle(stopping, broken, caller, value.ID, applied); err == nil || attempts != 1 {
			t.Fatalf("settle while stopping = %v after %d attempts", err, attempts)
		}
		if err := admission.Settle(stopping, write, caller, value.ID, applied); err != nil {
			t.Fatalf("last write while stopping = %v", err)
		}
		if outcome := inspect(value, body); outcome.Kind != "completed" {
			t.Fatalf("result written while stopping = %#v", outcome)
		}

		// A store that cannot confirm a commit is not written again.
		attempts = 0
		unconfirmed := func(context.Context, func(*sql.Conn) error) error {
			attempts++
			return &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "storage commit could not be confirmed"}
		}
		if err := admission.Settle(ctx, unconfirmed, caller, value.ID, applied); err == nil || attempts != 1 {
			t.Fatalf("settle on a failed store = %v after %d attempts", err, attempts)
		}
	})

	t.Run("pruning", func(t *testing.T) {
		if err := write(ctx, func(db *sql.Conn) error {
			_, err := db.ExecContext(ctx, "DELETE FROM requests")
			return err
		}); err != nil {
			t.Fatal(err)
		}
		now := time.Now()
		old := now.Add(-admission.Retention - time.Hour).Unix()
		type row struct {
			request protocol.Request
			body    []byte
			kept    bool
		}
		rows := map[string]*row{"completed": {}, "unknown": {}, "running": {kept: true}, "recent": {kept: true}, "unstamped": {kept: true}, "dispatch job": {kept: true}}
		for name, row := range rows {
			row.request, row.body = request()
			admit(row.request, row.body)
			if name != "running" && name != "unknown" {
				if err := admission.Settle(ctx, write, caller, row.request.ID, applied); err != nil {
					t.Fatal(err)
				}
			}
			at := old
			switch name {
			case "recent":
				at = now.Add(-admission.Retention + time.Hour).Unix()
			case "unstamped":
				at = 0
			}
			if err := write(ctx, func(db *sql.Conn) error {
				if name == "unknown" {
					if _, err := db.ExecContext(ctx, "UPDATE requests SET status='unknown' WHERE id=?1", row.request.ID.String()); err != nil {
						return err
					}
				}
				if name == "dispatch job" {
					body, err := json.Marshal(map[string]string{"request": row.request.ID.String()})
					if err != nil {
						return err
					}
					if _, err := db.ExecContext(ctx, "INSERT INTO dispatch_jobs(id,package,queue,status,body,callback,reference) VALUES('job','package','queue','completed',?1,X'',X'')", body); err != nil {
						return err
					}
				}
				_, err := db.ExecContext(ctx, "UPDATE requests SET at=?2 WHERE id=?1", row.request.ID.String(), at)
				return err
			}); err != nil {
				t.Fatal(err)
			}
		}
		for pass, want := range []int64{2, 0} {
			var removed int64
			if err := write(ctx, func(db *sql.Conn) error {
				var err error
				removed, err = admission.Prune(ctx, db, now)
				return err
			}); err != nil || removed != want {
				t.Fatalf("pass %d removed %d, %v; want %d", pass, removed, err, want)
			}
		}
		for name, row := range rows {
			outcome := inspect(row.request, row.body)
			if kept := outcome.Kind != "not_admitted"; kept != row.kept {
				t.Fatalf("%s kept = %t (%s)", name, kept, outcome.Kind)
			}
		}
		// The row written without a time now has one and expires a Retention later.
		var removed int64
		if err := write(ctx, func(db *sql.Conn) error {
			var err error
			removed, err = admission.Prune(ctx, db, now.Add(admission.Retention+time.Hour))
			return err
		}); err != nil || removed != 2 {
			t.Fatalf("later pass removed %d, %v", removed, err)
		}
		if outcome := inspect(rows["unstamped"].request, rows["unstamped"].body); outcome.Kind != "not_admitted" {
			t.Fatalf("unstamped after its retention = %#v", outcome)
		}
		if outcome := inspect(rows["dispatch job"].request, rows["dispatch job"].body); outcome.Kind != "completed" {
			t.Fatalf("request of a dispatch job = %#v", outcome)
		}
	})
}
