package live

import (
	"encoding/json"
	"math"
	"slices"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/sidebar"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestSidebarProjection(t *testing.T) {
	t.Parallel()
	node := protocol.NodeID{1}
	project := protocol.ProjectID{1}
	worktree := protocol.WorktreeID{1}
	hidden := protocol.ProjectID{2}
	ids := []protocol.SessionID{{1}, {2}, {3}, {4}, {5}, {6}, {7}}
	finished := []int64{20, 10}
	sessions := []protocol.Session{
		{ID: ids[0], Project: &project, CreationOrder: 3, Revision: 9, Activity: protocol.ActivitySummary{Title: "First", Attention: protocol.ActivityAttention{Revision: 12}}},
		{ID: ids[1], CreationOrder: 4, Activity: protocol.ActivitySummary{Title: "Chat"}},
		{ID: ids[2], Project: &project, CreationOrder: 1, Activity: protocol.ActivitySummary{Title: "Recent activity", Run: &protocol.ConversationRun{Status: protocol.ConversationStatusCompleted, FinishedMs: &finished[0]}, Attention: protocol.ActivityAttention{Unread: true}}},
		{ID: ids[3], Project: &project, CreationOrder: 2, Activity: protocol.ActivitySummary{Title: "Older activity", Run: &protocol.ConversationRun{Status: protocol.ConversationStatusRunning, StartedMs: &finished[1]}}},
		{ID: ids[4], Project: &project, CreationOrder: 5, Archived: true},
		{ID: ids[5], CreationOrder: 6},
		{ID: ids[6], Project: &hidden, CreationOrder: 7},
	}
	sessions[0].Worktree = worktree
	delegation := &protocol.Delegation{}
	sessions[5].Delegation = &delegation
	snapshot := protocol.Snapshot{Node: node, Projects: []protocol.Project{{ID: project, Name: "Workspace", Path: "/fixture/workspace", Appearance: protocol.ProjectsAppearance{Icon: "folder", Color: "none"}}}, Worktrees: []protocol.Worktree{{ID: worktree, Project: &project, Path: "/fixture/linked"}}, Sessions: sessions}
	prefs := preferences.Default()
	order := uint64(math.MaxUint64)
	pins := map[string]preferences.Pin{preferences.PinKey(node, ids[0].String()): {Pinned: true, Order: &order}, preferences.PinKey(node, ids[1].String()): {Pinned: true}, preferences.PinKey(protocol.NodeID{2}, ids[2].String()): {Pinned: true}}
	prefs.Pins = &pins
	d := Sidebar(snapshot, prefs)
	if d.Projects[0].Name != "Workspace" || d.Projects[0].Path != "/fixture/workspace" || d.Projects[0].Host != node.String() || d.Find(ids[0].String()).Revision != 9 || d.Find(ids[0].String()).AttentionRevision != 12 {
		t.Fatal("projection lost names, ownership or captured revisions")
	}
	if d.Find(ids[5].String()) != nil || d.Find(ids[6].String()) != nil {
		t.Fatal("hidden sessions entered desktop navigation")
	}
	if len(d.Models) != 0 || len(d.Providers) != 0 || len(d.Plugins) != 0 || d.Files != nil {
		t.Fatal("live projection contains fixture data")
	}
	if d.Projects[0].Trusted || d.Find(ids[0].String()).Worktree != "" || d.Find(ids[0].String()).WorktreePath != "/fixture/linked" {
		t.Fatal("live projection fabricated trust or a branch, or lost the actual worktree path")
	}
	if d.Find(ids[2].String()).Status != viewmodel.Unread || d.Find(ids[3].String()).Status != viewmodel.Running {
		t.Fatal("Client activity lanes did not reach presentation status")
	}
	want := []string{ids[1].String(), ids[0].String(), ids[2].String(), ids[3].String()}
	if got := sidebar.New(d, "").Order(); !slices.Equal(got, want) {
		t.Fatalf("sidebar order %v, want %v", got, want)
	}
	updated := "updated"
	prefs.SessionOrder = &updated
	d = Sidebar(snapshot, prefs)
	var shown []string
	for _, cv := range d.Projects[0].Conversations {
		if !cv.Archived {
			shown = append(shown, cv.ID)
		}
	}
	if !slices.Equal(shown, []string{ids[0].String(), ids[2].String(), ids[3].String()}) {
		t.Fatalf("updated order %v", shown)
	}
	if snapshot.Sessions[0].ID != ids[0] || prefs.Pin(node, ids[0].String()).Order == nil || *prefs.Pin(node, ids[0].String()).Order != math.MaxUint64 {
		t.Fatal("presentation changed authority order or full-width preference")
	}
}

func TestNoticesProjection(t *testing.T) {
	t.Parallel()
	t.Run("knowledge viewer", testKnowledgeViewer)
	node := protocol.NodeID{1}
	session := protocol.SessionID{1}
	times := []int64{100, 200}
	code := protocol.ErrorCodeInternal
	list := []client.Notice{
		{ID: client.NoticeID{Node: node, Cursor: 1, Target: client.NoticeTarget{Kind: "notification", Notification: protocol.NotificationID{1}}}, Kind: "notification", NotificationKind: protocol.NotificationKindInfo, Title: "Document", Message: "Description", TimestampMs: &times[0], Details: &protocol.NotificationDetails{Kind: "text", Data: &protocol.NotificationDetailsTextPayload{Body: "Full body"}}},
		{ID: client.NoticeID{Node: node, Cursor: 2, Target: client.NoticeTarget{Kind: "notification", Notification: protocol.NotificationID{2}}}, Kind: "failed", Title: "Private authored title", Message: "Private authored body", Error: &protocol.Fault{Message: "Private provider error"}, Session: &session, TimestampMs: &times[1], Details: &protocol.NotificationDetails{Kind: "task", Data: &protocol.NotificationDetailsTask{Turn: protocol.TurnID{1}, Status: protocol.ConversationStatusFailed, Error: &code}}},
		{ID: client.NoticeID{Node: node, Cursor: 3, Target: client.NoticeTarget{Kind: "session", Session: session}}, Kind: "approval", Session: &session},
	}
	notices := Notices(list)
	if len(notices) != 3 || notices[0].Kind != viewmodel.NoticeApproval || notices[0].TitleKey != "conversation" || notices[1].Kind != viewmodel.NoticeFailed || notices[2].DetailText != "Full body" || notices[2].Report != "" {
		t.Fatal("notice kind, order, fallback heading or text details changed")
	}
	if notices[1].Session != session.String() || notices[1].Time.UnixMilli() != 200 || notices[1].Details.Kind != "task" {
		t.Fatal("notice target or typed details were lost")
	}
	report := notices[1].Report
	if report == "" || strings.Contains(report, "Private") {
		t.Fatal("technical report omitted typed facts or included authored/provider text")
	}
	var decoded map[string]json.RawMessage
	if err := json.Unmarshal([]byte(report), &decoded); err != nil {
		t.Fatal(err)
	}
	var id map[string]json.RawMessage
	if err := json.Unmarshal(decoded["notification"], &id); err != nil {
		t.Fatal(err)
	}
	if id["node"] == nil || id["cursor"] == nil || id["target"] == nil {
		t.Fatal("technical report lost the portable notice identity")
	}
	// Delivery belongs to one notifier, and is independent of any App.View.
	prefs := &preferences.Store{Data: preferences.Default()}
	var jobs []func()
	var delivered []string
	n := &Notifier{prefs: prefs, seen: map[client.NoticeID]bool{}, pending: []client.Notice{list[1], list[2]}}
	n.SetUpdate(func(f func()) { jobs = append(jobs, f) })
	n.SetNotify(func(id, title, body, session string) { delivered = append(delivered, id) })
	prefs.Data.Notifications[1] = false
	for _, job := range jobs {
		job()
	}
	if !slices.Equal(delivered, []string{notices[1].ID}) {
		t.Fatalf("delivery without frames or category suppression failed: %v", delivered)
	}
}
