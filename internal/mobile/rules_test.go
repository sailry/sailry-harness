package mobile

import (
	"encoding/json"
	"maps"
	"testing"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestSyncSessionLanes(t *testing.T) {
	t.Parallel()
	type response struct {
		OK    bool                           `json:"ok"`
		Value map[string]client.ActivityLane `json:"value"`
		Error *protocol.Fault                `json:"error"`
	}
	call := func(t *testing.T, input []byte) response {
		t.Helper()
		data, synchronous := Sync("session_lanes", input)
		if !synchronous {
			t.Fatal("session lanes did not return a synchronous result")
		}
		var got response
		if err := json.Unmarshal(data, &got); err != nil {
			t.Fatalf("decode session lanes: %v", err)
		}
		return got
	}

	t.Run("activity projection", func(t *testing.T) {
		makeSession := func(index byte, status protocol.ConversationStatus, unread bool) protocol.Session {
			session := protocol.DefaultSession()
			session.ID = protocol.SessionID{index}
			session.Worktree = protocol.WorktreeID{index}
			session.Config.Mode, session.Config.Permission = protocol.WorkModeCode, protocol.PermissionAsk
			session.Config.Provider = protocol.ProviderID{1}
			run := protocol.DefaultConversationRun()
			run.Session, run.Worktree, run.Turn = session.ID, session.Worktree, protocol.TurnID{index}
			run.Kind, run.Status = protocol.ConversationRunKindTask, status
			session.Activity.Run = &run
			session.Activity.Attention.Unread = unread
			return session
		}

		parent := makeSession(1, protocol.ConversationStatusCompleted, true)
		child := makeSession(2, protocol.ConversationStatusRunning, false)
		origin := &protocol.Delegation{Session: parent.ID, Turn: parent.Activity.Run.Turn, Entry: "fixture"}
		child.Delegation = &origin
		waiting := makeSession(3, protocol.ConversationStatusRunning, false)
		waiting.Activity.Waiting = new(protocol.ActivityWaitingApproval)
		unreadFailure := makeSession(4, protocol.ConversationStatusFailed, true)
		readFailure := makeSession(5, protocol.ConversationStatusFailed, false)
		completed := makeSession(6, protocol.ConversationStatusCompleted, true)
		completed.Config.Mode = protocol.WorkMode("future_mode")
		sessions := []protocol.Session{parent, child, waiting, unreadFailure, readFailure, completed}
		input, err := json.Marshal(map[string][]protocol.Session{"sessions": sessions})
		if err != nil {
			t.Fatal(err)
		}

		want := map[string]client.ActivityLane{
			parent.ID.String():        client.ActivityRunning,
			child.ID.String():         client.ActivityRunning,
			waiting.ID.String():       client.ActivityWaiting,
			unreadFailure.ID.String(): client.ActivityFailed,
			readFailure.ID.String():   client.ActivityIdle,
			completed.ID.String():     client.ActivityCompleted,
		}
		got := call(t, input)
		if !got.OK || !maps.Equal(got.Value, want) {
			t.Fatalf("session lanes = %+v, want %v", got, want)
		}
	})

	t.Run("empty array", func(t *testing.T) {
		got := call(t, []byte(`{"sessions":[]}`))
		if !got.OK || got.Value == nil || len(got.Value) != 0 {
			t.Fatalf("empty session lanes = %+v, want an empty object", got)
		}
	})
	for _, input := range []string{`{}`, `{"sessions":null}`, `{"sessions":"[]"}`, `{"sessions":{}}`, `{"sessions":[null]}`, `{"sessions":[{}]}`} {
		t.Run(input, func(t *testing.T) {
			got := call(t, []byte(input))
			if got.OK || got.Error == nil || got.Error.Code != protocol.ErrorCodeInvalidRequest {
				t.Fatalf("invalid sessions = %+v", got)
			}
		})
	}
}
