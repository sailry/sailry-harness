package node

import (
	"database/sql"
	"encoding/json"
	"fmt"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/jonboulle/clockwork"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestDispatchTickIsolatesUsersAndRetriesTheirInitialRecovery(t *testing.T) {
	t.Parallel()
	ctx := t.Context()
	profile, err := OpenProfile(ctx, filepath.Join(t.TempDir(), "profile"))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := profile.Close(); err != nil {
			t.Error(err)
		}
	})
	clock := clockwork.NewFakeClockAt(time.UnixMilli(1000))
	// No runner is started: this fixture controls every tick and clock advance.
	node := &Node{ctx: ctx, profile: profile, dispatch: &dispatchControls{clock: clock}}
	var users []protocol.User
	var badJob protocol.JobID
	var badSchedule protocol.ScheduleID
	if err := profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		first, err := store.LocalUser(ctx, db)
		if err != nil {
			return err
		}
		bad, err := store.CreateUser(ctx, db, "Damaged dispatch records")
		if err != nil {
			return err
		}
		last, err := store.CreateUser(ctx, db, "Later healthy user")
		if err != nil {
			return err
		}
		users = []protocol.User{first, bad, last}
		for index, user := range users {
			owner := store.WithUser(ctx, user)
			info := protocol.DefaultPluginInfo()
			info.Summary.Name, info.Summary.Digest = "dispatch-isolation", strings.Repeat("a", 64)
			version := "0.1.0"
			info.Summary.Version = &version
			info, err = store.InstallPlugin(owner, db, info, 0, true)
			if err != nil {
				return err
			}
			reference := plugins.PackageReference(info)
			handler := protocol.DispatchHandler{
				Name: "fixture", Enabled: true, Queue: "fixture",
				Source: protocol.DispatchSource{Package: reference.Name, Topic: "fixture"},
				Callback: protocol.DispatchCallback{
					Completion: protocol.DispatchCompletionCommand, Bindings: map[string]string{},
					Command: &protocol.Command{Kind: "read_plugin_value", Data: &protocol.CommandReadPluginValuePayload{Key: "fixture"}},
				},
			}
			if _, _, err := store.ExecuteDispatch(owner, db, node.ID(), reference, protocol.DispatchCommand{Kind: "save_handler", Data: handler}, 100); err != nil {
				return err
			}
			schedule := protocol.DispatchSchedule{
				ID: protocol.NewScheduleID(), Enabled: true, Topic: "fixture", Payload: json.RawMessage("null"),
				Timing: protocol.DispatchTiming{Kind: "once", Data: protocol.DispatchTimingOncePayload{AtMs: 1000}},
			}
			if _, _, err := store.ExecuteDispatch(owner, db, node.ID(), reference, protocol.DispatchCommand{Kind: "save_schedule", Data: schedule}, 100); err != nil {
				return err
			}
			if index == 1 {
				// Leave one interrupted callback for recovery. A later malformed
				// schedule makes its first recovery transaction roll back.
				batch, _, err := store.DispatchTick(owner, db, node.ID(), 1000, false)
				if err != nil {
					return err
				}
				if len(batch.Work) != 1 {
					return fmt.Errorf("bad user fixture has %d jobs", len(batch.Work))
				}
				badJob, badSchedule = batch.Work[0].Job, schedule.ID
				if _, err := db.ExecContext(ctx, "UPDATE dispatch_user_schedules SET next_ms=1000,body=?2 WHERE user=?1 AND id=?3", bad.ID.String(), []byte("{"), schedule.ID.String()); err != nil {
					return err
				}
			}
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	batch, err := node.dispatchTick(ctx, true)
	if err != nil || len(batch.Work) != 2 || batch.Pending || batch.NextMs == nil || *batch.NextMs != 2000 {
		t.Fatalf("one user's error suppressed healthy work or omitted retry: batch=%+v err=%v", batch, err)
	}
	for index, expected := range []protocol.User{users[0], users[2]} {
		if batch.Work[index].User.ID != expected.ID || batch.Work[index].StorageUser != expected.ID {
			t.Fatalf("healthy callback lost its captured owner: %+v", batch.Work[index])
		}
	}
	healthyJobs := []protocol.JobID{batch.Work[0].Job, batch.Work[1].Job}
	status := func(job protocol.JobID) protocol.DispatchStatus {
		t.Helper()
		var value protocol.DispatchStatus
		if err := profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			return db.QueryRowContext(ctx, "SELECT status FROM dispatch_jobs WHERE id=?1", job.String()).Scan(&value)
		}); err != nil {
			t.Fatal(err)
		}
		return value
	}
	for _, job := range append(healthyJobs, badJob) {
		if current := status(job); current != protocol.DispatchStatusRunning {
			t.Fatalf("committed claim or rolled-back recovery changed: job=%s status=%s", job, current)
		}
	}
	if err := profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		_, err := db.ExecContext(ctx, "DELETE FROM dispatch_user_schedules WHERE user=?1 AND id=?2", users[1].ID.String(), badSchedule.String())
		return err
	}); err != nil {
		t.Fatal(err)
	}
	batch, err = node.dispatchTick(ctx, false)
	if err != nil || len(batch.Work) != 0 || batch.Pending || batch.NextMs == nil || *batch.NextMs != 2000 || status(badJob) != protocol.DispatchStatusRunning {
		t.Fatalf("failed user retried without the error wait: batch=%+v err=%v", batch, err)
	}
	clock.Advance(time.Second)
	batch, err = node.dispatchTick(ctx, false)
	if err != nil || len(batch.Work) != 0 || batch.Pending || batch.NextMs != nil {
		t.Fatalf("recovery replayed callbacks or kept a stale retry: batch=%+v err=%v", batch, err)
	}
	if current := status(badJob); current != protocol.DispatchStatusUnknown {
		t.Fatalf("failed initial recovery was permanently skipped: %s", current)
	}
	for _, job := range healthyJobs {
		if current := status(job); current != protocol.DispatchStatusRunning {
			t.Fatalf("successful user's recovery ran a second time: job=%s status=%s", job, current)
		}
	}
}
