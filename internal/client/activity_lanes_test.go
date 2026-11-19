package client

import (
	"reflect"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func activitySession(index byte, status protocol.ConversationStatus) protocol.Session {
	session := projectionSession(index, uint64(index))
	session.Activity.Queued = 2
	if status != "" {
		run := protocol.DefaultConversationRun()
		run.Session, run.Worktree, run.Turn = session.ID, session.Worktree, protocol.TurnID{index}
		run.Status = status
		session.Activity.Run = &run
	}
	return session
}

func activityChild(index byte, parent protocol.Session, status protocol.ConversationStatus) protocol.Session {
	child := activitySession(index, status)
	child.Project = parent.Project
	origin := protocol.Delegation{Session: parent.ID}
	captured := &origin
	child.Delegation = &captured
	return child
}

func testActivityLanes(t *testing.T) {
	t.Run("execution and terminal reports", func(t *testing.T) {
		for _, scenario := range []struct {
			status protocol.ConversationStatus
			unread bool
			wait   bool
			lane   ActivityLane
		}{
			{"", false, false, ActivityIdle},
			{protocol.ConversationStatusQueued, false, false, ActivityIdle},
			{protocol.ConversationStatusCancelled, false, false, ActivityIdle},
			{protocol.ConversationStatusContinued, true, false, ActivityIdle},
			{protocol.ConversationStatusRunning, false, false, ActivityRunning},
			{protocol.ConversationStatusRunning, false, true, ActivityWaiting},
			{protocol.ConversationStatusStopping, false, true, ActivityRunning},
			{protocol.ConversationStatusCompleted, false, false, ActivityIdle},
			{protocol.ConversationStatusCompleted, true, false, ActivityCompleted},
			{protocol.ConversationStatusFailed, false, false, ActivityIdle},
			{protocol.ConversationStatusFailed, true, false, ActivityFailed},
			{protocol.ConversationStatusInterrupted, true, false, ActivityFailed},
		} {
			session := activitySession(1, scenario.status)
			session.Activity.Attention.Unread = scenario.unread
			if scenario.wait {
				session.Activity.Waiting = new(protocol.ActivityWaitingApproval)
			}
			if actual := activityLane(LaneOf(session)); actual != scenario.lane || session.Activity.Queued != 2 {
				t.Fatalf("status %s, unread %v, waiting %v: %s", scenario.status, scenario.unread, scenario.wait, actual)
			}
		}
		for _, scenario := range []struct {
			status   protocol.TerminalStatus
			activity *protocol.TerminalActivityReport
			lane     ActivityLane
			present  bool
		}{
			{protocol.TerminalStatus{Kind: "running"}, nil, ActivityIdle, true},
			{protocol.TerminalStatus{Kind: "running"}, &protocol.TerminalActivityReport{State: protocol.TerminalActivityWorking}, ActivityRunning, true},
			{protocol.TerminalStatus{Kind: "running"}, &protocol.TerminalActivityReport{State: protocol.TerminalActivityWaitingForInput}, ActivityWaiting, true},
			{protocol.TerminalStatus{Kind: "running"}, &protocol.TerminalActivityReport{State: protocol.TerminalActivityCompleted}, ActivityCompleted, true},
			{protocol.TerminalStatus{Kind: "exited", Data: protocol.TerminalStatusExitedPayload{Code: 0}}, nil, ActivityCompleted, true},
			{protocol.TerminalStatus{Kind: "exited", Data: protocol.TerminalStatusExitedPayload{Code: 1}}, nil, ActivityFailed, true},
			{protocol.TerminalStatus{Kind: "failed"}, nil, ActivityFailed, true},
			{protocol.TerminalStatus{Kind: "stopped"}, nil, ActivityIdle, true},
			{protocol.TerminalStatus{Kind: "closed"}, nil, ActivityIdle, false},
		} {
			terminal := protocol.DefaultTerminalInfo()
			terminal.Status, terminal.Activity = scenario.status, scenario.activity
			lane, present := TerminalActivity(terminal)
			if lane != scenario.lane || present != scenario.present {
				t.Fatalf("terminal %s: %s, %v", scenario.status.Kind, lane, present)
			}
		}
	})
	t.Run("owned descendants and cycles", func(t *testing.T) {
		parent := activitySession(1, protocol.ConversationStatusCompleted)
		parent.Activity.Attention.Unread = true
		intermediate := activityChild(2, parent, protocol.ConversationStatusCompleted)
		child := activityChild(3, intermediate, protocol.ConversationStatusRunning)
		graph := []protocol.Session{parent, intermediate, child}
		if AggregateLanes(graph)[0] != LaneRunning || activityLane(LaneOf(parent)) != ActivityCompleted {
			t.Fatal("nested child changed the parent's canonical result or lost its activity")
		}
		child.Activity.Waiting = new(protocol.ActivityWaitingInput)
		graph[2] = child
		if AggregateLanes(graph)[0] != LaneWaiting {
			t.Fatal("nested waiting input did not project to its owner")
		}
		parent.Activity.Background, intermediate.Activity.Background = 1, 1
		if LaneOf(parent) != LaneRunning || AggregateLanes([]protocol.Session{parent, intermediate, child})[0] != LaneWaiting {
			t.Fatal("background activity hid a descendant waiting for input")
		}
		if AggregateLanes([]protocol.Session{parent, intermediate})[0] != LaneRunning {
			t.Fatal("background follow-up did not keep the parent active")
		}
		parent.Activity.Run.Status = protocol.ConversationStatusRunning
		parent.Activity.Waiting = new(protocol.ActivityWaitingApproval)
		child.Activity.Waiting = nil
		if AggregateLanes([]protocol.Session{parent, intermediate, child})[0] != LaneWaiting {
			t.Fatal("child overrode its owner's active lane")
		}
		parent = activitySession(1, protocol.ConversationStatusCompleted)
		misplaced := activityChild(4, parent, protocol.ConversationStatusRunning)
		misplaced.Project = new(protocol.ProjectID{1})
		cyclic := activityChild(5, parent, protocol.ConversationStatusRunning)
		(*cyclic.Delegation).Session = cyclic.ID
		stranger := activityChild(6, activitySession(7, protocol.ConversationStatusRunning), protocol.ConversationStatusRunning)
		if AggregateLanes([]protocol.Session{parent, misplaced, cyclic, stranger})[0] != LaneIdle {
			t.Fatal("unrelated or cyclic ancestry escaped the supplied project graph")
		}
		archived := activityChild(8, parent, protocol.ConversationStatusRunning)
		archived.Archived = true
		if AggregateLanes([]protocol.Session{parent, archived})[0] != LaneRunning {
			t.Fatal("controller navigation filtering changed the Node's owned descendant activity")
		}
	})
	t.Run("shared order and controller terminal attention", func(t *testing.T) {
		idle := activitySession(1, "")
		running := activitySession(2, protocol.ConversationStatusRunning)
		waiting := activitySession(3, protocol.ConversationStatusRunning)
		waiting.Activity.Waiting = new(protocol.ActivityWaitingInput)
		older := activitySession(4, protocol.ConversationStatusCompleted)
		older.Activity.Attention.Unread, older.Activity.Run.FinishedMs = true, new(int64(10))
		newer := activitySession(5, protocol.ConversationStatusFailed)
		newer.Activity.Attention.Unread, newer.Activity.Run.StartedMs = true, new(int64(20))
		unknown := activitySession(6, protocol.ConversationStatusCompleted)
		unknown.Activity.Attention.Unread = true
		terminal := protocol.DefaultTerminalInfo()
		terminal.ID = protocol.TerminalID{1}
		terminal.Status = protocol.TerminalStatus{Kind: "exited", Data: protocol.TerminalStatusExitedPayload{Code: 0}}
		catalog := protocol.DefaultActivityCatalog()
		catalog.Sessions = []protocol.Session{idle, unknown, running, older, waiting, newer}
		catalog.Terminals = []protocol.TerminalInfo{terminal}
		want := []string{
			"session:" + waiting.ID.String(), "session:" + newer.ID.String(), "session:" + older.ID.String(),
			"session:" + unknown.ID.String(), "terminal:" + terminal.ID.String(), "session:" + running.ID.String(),
			"session:" + idle.ID.String(),
		}
		projection := ProjectActivity(catalog, []protocol.TerminalID{terminal.ID})
		if !reflect.DeepEqual(projection.ActivityOrder, want) {
			t.Fatalf("shared attention order: %v", projection.ActivityOrder)
		}
		read := ProjectActivity(catalog, nil)
		if read.TerminalLanes[terminal.ID.String()] != ActivityCompleted || read.ActivityOrder[len(read.ActivityOrder)-1] != "terminal:"+terminal.ID.String() {
			t.Fatal("read terminal changed its actual lane or retained unread display precedence")
		}
		controller := New(&clientTransport{})
		first, second := protocol.TerminalID{1}, protocol.TerminalID{2}
		controller.feed.notices = []Notice{
			{ID: NoticeID{Node: controller.Target(), Target: NoticeTarget{Kind: "terminal", Terminal: second}}},
			{ID: NoticeID{Node: controller.Target(), Target: NoticeTarget{Kind: "terminal", Terminal: first}}},
			{ID: NoticeID{Node: controller.Target(), Target: NoticeTarget{Kind: "terminal", Terminal: second}}},
			{ID: NoticeID{Node: protocol.NodeID{2}, Target: NoticeTarget{Kind: "terminal", Terminal: protocol.TerminalID{3}}}},
			{ID: NoticeID{Node: controller.Target(), Target: NoticeTarget{Kind: "terminal", Terminal: protocol.TerminalID{4}}}, Read: true},
		}
		if unread := controller.UnreadTerminals(); !reflect.DeepEqual(unread, []protocol.TerminalID{first, second}) {
			t.Fatalf("captured Node unread terminal identities: %v", unread)
		}
	})
}

func TestAggregateLanesAcrossActiveSiblings(t *testing.T) {
	root := activitySession(1, "")
	owner := activityChild(2, root, protocol.ConversationStatusRunning)
	owner.Activity.Waiting = new(protocol.ActivityWaitingApproval)
	worker := activityChild(3, owner, protocol.ConversationStatusRunning)
	background := activityChild(4, root, protocol.ConversationStatusCompleted)
	background.Activity.Background = 1
	for _, sessions := range [][]protocol.Session{{root, background, owner, worker}, {worker, owner, background, root}} {
		lanes := AggregateLanes(sessions)
		for i, session := range sessions {
			want := LaneRunning
			if session.ID == owner.ID {
				want = LaneWaiting
			}
			if lanes[i] != want {
				t.Fatalf("session %v: got %v, want %v", session.ID, lanes[i], want)
			}
		}
	}
}

func BenchmarkProjectActivityReview(b *testing.B) {
	var catalog protocol.ActivityCatalog
	for i := range 160 {
		catalog.Sessions = append(catalog.Sessions, activitySession(byte(i), protocol.ConversationStatusCompleted))
	}
	for i := range 16 {
		catalog.Sessions = append(catalog.Sessions, activityChild(byte(160+i), catalog.Sessions[i], protocol.ConversationStatusRunning))
	}
	b.ReportAllocs()
	for b.Loop() {
		ProjectActivity(catalog, nil)
	}
}
