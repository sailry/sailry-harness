package client

import (
	"reflect"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func projectionSnapshot(cursor uint64) protocol.Snapshot {
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node, snapshot.Cursor = protocol.NodeID{1}, cursor
	return snapshot
}

func projectionSession(index byte, order uint64) protocol.Session {
	session := protocol.DefaultSession()
	session.ID, session.Worktree, session.CreationOrder = protocol.SessionID{index}, protocol.WorktreeID{index}, order
	return session
}

func projectionEvent(cursor uint64, kind string, data any) protocol.Update {
	return protocol.Update{Kind: "event", Data: protocol.EventEnvelope{Node: protocol.NodeID{1}, Cursor: cursor, Event: protocol.Event{Kind: kind, Data: data}}}
}

func mustApply(t *testing.T, projection *Projection, generation uint64, update protocol.Update, expected Apply) {
	t.Helper()
	actual, err := projection.Apply(generation, update)
	if err != nil || actual != expected {
		t.Fatalf("apply %s = %v, %v; want %v", update.Kind, actual, err, expected)
	}
}

func TestProjectionOrder(t *testing.T) {
	t.Parallel()
	t.Run("sessions_keep_authority_order", func(t *testing.T) {
		first, second, created := projectionSession(1, 1), projectionSession(2, 2), projectionSession(3, 3)
		snapshot := projectionSnapshot(0)
		snapshot.Sessions = []protocol.Session{second, first}
		for _, session := range []protocol.Session{first, second, created} {
			snapshot.Worktrees = append(snapshot.Worktrees, protocol.Worktree{ID: session.Worktree})
		}
		projection := NewProjection(snapshot.Node, 1)
		mustApply(t, projection, 1, protocol.Update{Kind: "snapshot", Data: snapshot}, Applied)
		first.Activity.Title = "Renamed"
		mustApply(t, projection, 1, projectionEvent(1, "session_changed", &first), Applied)
		mustApply(t, projection, 1, projectionEvent(2, "session_changed", created), Applied)
		if got := projection.Snapshot().Sessions; !reflect.DeepEqual(got, []protocol.Session{created, second, first}) {
			t.Fatalf("session order: %v", got)
		}
		order := []protocol.SessionID{first.ID, created.ID, second.ID}
		mustApply(t, projection, 1, projectionEvent(3, "sessions_reordered", &order), Applied)
		if got := projection.Snapshot().Sessions; !reflect.DeepEqual(got, []protocol.Session{first, created, second}) {
			t.Fatal("manual order did not follow authority")
		}
		mustApply(t, projection, 1, projectionEvent(4, "sessions_removed", []protocol.SessionID{created.ID}), Applied)
		if got := projection.Snapshot().Sessions; !reflect.DeepEqual(got, []protocol.Session{first, second}) {
			t.Fatal("removed session remained in projection")
		}
		mustApply(t, projection, 1, projectionEvent(5, "sessions_reordered", []protocol.SessionID{first.ID, first.ID}), Recover)
	})
	t.Run("recent_filters_and_stable_creation_order", func(t *testing.T) {
		snapshot := projectionSnapshot(0)
		active := protocol.ProjectID{1}
		removed := protocol.ProjectID{2}
		snapshot.Projects = []protocol.Project{{ID: active}}
		first, second := projectionSession(1, 0), projectionSession(2, 0)
		older := projectionSession(3, 1)
		older.Project = &active
		newer := projectionSession(4, 3)
		hidden := projectionSession(5, 4)
		hidden.Project = &removed
		archived := projectionSession(6, 5)
		archived.Archived = true
		delegated := projectionSession(7, 6)
		delegation := &protocol.Delegation{}
		delegated.Delegation = &delegation
		snapshot.Sessions = []protocol.Session{second, first, older, newer, hidden, archived, delegated}
		if got := Recent(snapshot); !reflect.DeepEqual(got, []protocol.Session{newer, older, second, first}) {
			t.Fatalf("recent sessions: %v", got)
		}
	})
	t.Run("notifications_and_revisions", func(t *testing.T) {
		projection := NewProjection(protocol.NodeID{1}, 1)
		mustApply(t, projection, 1, protocol.Update{Kind: "snapshot", Data: projectionSnapshot(0)}, Applied)
		first := protocol.NotificationNotice{ID: protocol.NotificationID{1}, Sequence: 1}
		second := protocol.NotificationNotice{ID: protocol.NotificationID{2}, Sequence: 2}
		mustApply(t, projection, 1, projectionEvent(1, "notification_changed", first), Applied)
		mustApply(t, projection, 1, projectionEvent(2, "notification_changed", second), Applied)
		mustApply(t, projection, 1, projectionEvent(3, "notifications_read", protocol.EventNotificationsReadPayload{IDs: []protocol.NotificationID{first.ID}}), Applied)
		mustApply(t, projection, 1, projectionEvent(4, "power_settings_changed", protocol.EventPowerSettingsChangedPayload{Revision: 8}), Applied)
		state := projection.Snapshot()
		if len(state.Notifications) != 2 || state.Notifications[0].ID != second.ID || !state.Notifications[1].Read || state.PowerSettingsRevision != 8 {
			t.Fatal("notification order/read state or settings revision was lost")
		}
		mustApply(t, projection, 1, projectionEvent(5, "notifications_dismissed", protocol.EventNotificationsDismissedPayload{IDs: []protocol.NotificationID{first.ID}}), Applied)
		if got := projection.Snapshot().Notifications; len(got) != 1 || got[0].ID != second.ID {
			t.Fatal("dismissed notice remained")
		}
	})
	t.Run("project_and_reserved_worktree_recovery", func(t *testing.T) {
		for _, update := range []protocol.Update{
			projectionEvent(1, "project_registered", protocol.Project{ID: protocol.ProjectID{1}}),
			projectionEvent(1, "session_changed", projectionSession(1, 1)),
		} {
			projection := NewProjection(protocol.NodeID{1}, 1)
			mustApply(t, projection, 1, protocol.Update{Kind: "snapshot", Data: projectionSnapshot(0)}, Applied)
			mustApply(t, projection, 1, update, Recover)
			if projection.Snapshot().Cursor != 0 {
				t.Fatal("recovery advanced a partial projection")
			}
		}
	})
}

func TestProjectionDedup(t *testing.T) {
	t.Parallel()
	projection := NewProjection(protocol.NodeID{1}, 1)
	mustApply(t, projection, 1, protocol.Update{Kind: "snapshot", Data: projectionSnapshot(0)}, Applied)
	change := projectionEvent(1, "defaults_changed", protocol.Defaults{Revision: 7})
	mustApply(t, projection, 1, change, Applied)
	mustApply(t, projection, 1, change, Ignored)
	mustApply(t, projection, 1, protocol.Update{Kind: "snapshot", Data: projectionSnapshot(0)}, Ignored)
	if projection.Snapshot().Defaults.Revision != 7 {
		t.Fatal("duplicate or stale snapshot replaced newer state")
	}
	for _, test := range []struct {
		name   string
		update protocol.Update
		code   protocol.ErrorCode
	}{
		{"wrong_node", protocol.Update{Kind: "snapshot", Data: protocol.Snapshot{Node: protocol.NodeID{2}}}, protocol.ErrorCodeWrongTarget},
		{"wrong_resource", protocol.Update{Kind: "status"}, protocol.ErrorCodeInvalidRequest},
		{"wrong_payload", protocol.Update{Kind: "event", Data: "invalid"}, protocol.ErrorCodeInvalidRequest},
	} {
		t.Run(test.name, func(t *testing.T) {
			if _, err := projection.Apply(1, test.update); faultCode(err) != test.code {
				t.Fatalf("error %v; want %s", err, test.code)
			}
			if projection.Snapshot().Cursor != 1 {
				t.Fatal("invalid update mutated the projection")
			}
		})
	}
	mustApply(t, projection, 1, projectionEvent(3, "defaults_changed", protocol.Defaults{Revision: 9}), Recover)
	mustApply(t, projection, 1, projectionEvent(2, "defaults_changed", protocol.Defaults{Revision: 8}), Recover)
	if err := projection.Reconnect(2); err != nil {
		t.Fatal(err)
	}
	mustApply(t, projection, 1, protocol.Update{Kind: "snapshot", Data: projectionSnapshot(99)}, Ignored)
	if err := projection.Reconnect(2); faultCode(err) != protocol.ErrorCodeInvalidRequest {
		t.Fatalf("repeated generation: %v", err)
	}
	mustApply(t, projection, 2, protocol.Update{Kind: "snapshot", Data: projectionSnapshot(0)}, Applied)
	if projection.Snapshot().Cursor != 0 {
		t.Fatal("reconnect rejected restored authority cursor")
	}
	mustApply(t, projection, 2, protocol.Update{Kind: "reset_required"}, Recover)
	mustApply(t, projection, 2, protocol.Update{Kind: "snapshot", Data: projectionSnapshot(5)}, Applied)
	state := projection.Snapshot()
	state.Defaults.Revision = 123
	if projection.Snapshot().Defaults.Revision == 123 {
		t.Fatal("snapshot reader mutated the shared projection")
	}
}
