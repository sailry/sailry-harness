package live

import (
	"encoding/json"
	"math"
	"path/filepath"
	"slices"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/sidebar"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestSidebarProjection(t *testing.T) {
	t.Run("session order command", testSessionOrderCommand)
	t.Parallel()
	node := protocol.NodeID{1}
	project := protocol.ProjectID{1}
	worktree := protocol.WorktreeID{1}
	hidden := protocol.ProjectID{2}
	second := protocol.ProjectID{3}
	ids := []protocol.SessionID{{1}, {2}, {3}, {4}, {5}, {6}, {7}, {8}}
	finished := []int64{20, 10, 5}
	sessions := []protocol.Session{
		{ID: ids[0], Project: &project, CreationOrder: 3, Revision: 9, Activity: protocol.ActivitySummary{Title: "First", Attention: protocol.ActivityAttention{Revision: 12}}},
		{ID: ids[1], CreationOrder: 4, Activity: protocol.ActivitySummary{Title: "Chat"}},
		{ID: ids[2], Project: &project, CreationOrder: 1, Activity: protocol.ActivitySummary{Title: "Recent activity", Run: &protocol.ConversationRun{Status: protocol.ConversationStatusCompleted, FinishedMs: &finished[0]}, Attention: protocol.ActivityAttention{Unread: true}}},
		{ID: ids[3], Project: &project, CreationOrder: 2, Activity: protocol.ActivitySummary{Title: "Older activity", Run: &protocol.ConversationRun{Status: protocol.ConversationStatusRunning, StartedMs: &finished[1]}}},
		{ID: ids[4], Project: &project, CreationOrder: 5, Archived: true},
		{ID: ids[5], CreationOrder: 6},
		{ID: ids[6], Project: &hidden, CreationOrder: 7},
		{ID: ids[7], Project: &second, CreationOrder: 8, Activity: protocol.ActivitySummary{Title: "Zeta", Run: &protocol.ConversationRun{Status: protocol.ConversationStatusCompleted, FinishedMs: &finished[2]}}},
	}
	sessions[0].Worktree = worktree
	delegation := &protocol.Delegation{}
	sessions[5].Delegation = &delegation
	snapshot := protocol.Snapshot{Node: node, Projects: []protocol.Project{{ID: project, Name: "Workspace", Path: "/fixture/workspace", Appearance: protocol.ProjectsAppearance{Icon: "folder", Color: "none"}}, {ID: second, Name: "atlas", Path: "/fixture/atlas"}}, Worktrees: []protocol.Worktree{{ID: worktree, Project: &project, Path: "/fixture/linked"}}, Sessions: sessions}
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
	if d.Find(ids[0].String()).Worktree != "" || d.Find(ids[0].String()).WorktreePath != "/fixture/linked" {
		t.Fatal("live projection fabricated a branch, or lost the actual worktree path")
	}
	if d.Find(ids[2].String()).Status != viewmodel.Unread || d.Find(ids[3].String()).Status != viewmodel.Running {
		t.Fatal("Client activity lanes did not reach presentation status")
	}
	// The projection keeps the Node's order whatever the preference; the
	// sidebar sorts it. Pins lead in every order: the legacy pin, then the
	// ordered one. Then the projects, each with its sessions: the Node lists
	// Workspace before atlas, whose only run is the oldest.
	name := func(id protocol.SessionID) string { return id.String() }
	for _, c := range []struct {
		preference string
		want       []string
	}{
		// A profile that never chose is updated: what never ran first, then
		// the last run, newest first; Workspace ran last.
		{"", []string{name(ids[1]), name(ids[0]), name(ids[2]), name(ids[3]), name(ids[7])}},
		{"updated", []string{name(ids[1]), name(ids[0]), name(ids[2]), name(ids[3]), name(ids[7])}},
		// Created: the newest project first, sessions in the Node's order.
		{"created", []string{name(ids[1]), name(ids[0]), name(ids[7]), name(ids[2]), name(ids[3])}},
		// Name: atlas, then Workspace with First, Older activity, Recent activity.
		{"name", []string{name(ids[1]), name(ids[0]), name(ids[7]), name(ids[3]), name(ids[2])}},
	} {
		prefs.SessionOrder = nil
		if c.preference != "" {
			prefs.SessionOrder = &c.preference
		}
		if err := prefs.Validate(); err != nil {
			t.Fatalf("session order %q: %v", c.preference, err)
		}
		d = Sidebar(snapshot, prefs)
		var shown []string
		for _, cv := range d.Projects[0].Conversations {
			if !cv.Archived {
				shown = append(shown, cv.ID)
			}
		}
		if d.Projects[0].Name != "Workspace" || !slices.Equal(shown, []string{name(ids[0]), name(ids[2]), name(ids[3])}) {
			t.Fatalf("%q: the projection left the Node's order: %v", c.preference, shown)
		}
		state := sidebar.New(d, "")
		*state.Sort = int(sidebar.SortOf(c.preference))
		if got := state.Order(); !slices.Equal(got, c.want) {
			t.Fatalf("%q: sidebar order %v, want %v", c.preference, got, c.want)
		}
	}
	t.Run("an unset update channel follows the running build", func(t *testing.T) {
		stable, experience := "stable", "experience"
		for _, current := range []struct {
			saved   *string
			version string
			want    int
		}{
			{nil, "0.1.6", 0}, {nil, "0.2.0-beta.1", 1}, {nil, "not a version", 0},
			{&stable, "0.2.0-beta.1", 0}, {&experience, "0.1.6", 1},
		} {
			if got := updateChannel(current.saved, current.version); got != current.want {
				t.Errorf("saved %v on %s: channel %d, want %d", current.saved, current.version, got, current.want)
			}
		}
		r := &Runtime{ctx: t.Context(), App: shell.NewLive(Sidebar(snapshot, preferences.Default())), Preferences: preferences.Open(filepath.Join(t.TempDir(), "preferences.json"))}
		r.applyPreferences()
		initial := updateChannel(nil, link.ProductVersion)
		r.savePreferences()
		r.workers.Wait()
		if r.App.Settings.Model.Channel != initial || r.Preferences.Data.UpdateChannel != nil {
			t.Fatalf("a profile that never chose shows channel %d and saved %v", r.App.Settings.Model.Channel, r.Preferences.Data.UpdateChannel)
		}
		// About shows the channel in use, which then stays this profile's.
		r.App.Page, r.App.Settings.Section = shell.SettingsPage, settings.About
		r.savePreferences()
		r.workers.Wait()
		if saved := r.Preferences.Data.UpdateChannel; saved == nil || *saved != updateChannels[initial] {
			t.Fatalf("the channel About showed was saved as %v", saved)
		}
		r.App.Settings.Model.Channel = 1 - initial
		r.savePreferences()
		r.workers.Wait()
		if saved := r.Preferences.Data.UpdateChannel; saved == nil || *saved != updateChannels[1-initial] {
			t.Fatalf("the chosen channel was saved as %v", saved)
		}
	})
	t.Run("saved developer switches do not enter demo presentation", func(t *testing.T) {
		store := preferences.Open(filepath.Join(t.TempDir(), "preferences.json"))
		store.Data.DemoMode, store.Data.TestMode = true, true
		r := &Runtime{ctx: t.Context(), App: shell.NewLive(Sidebar(snapshot, store.Data)), Preferences: store}
		r.applyPreferences()
		if r.App.Settings.Model.DemoMode || r.App.Demo() {
			t.Fatal("a saved developer switch enabled demo presentation")
		}
		// A normal settings save keeps the old data without turning a launch
		// mode into a user preference again.
		r.App.Settings.Model.SidebarMetrics = !r.App.Settings.Model.SidebarMetrics
		r.savePreferences()
		r.workers.Wait()
		stored := preferences.Open(store.Path).Data
		if !stored.DemoMode || !stored.TestMode {
			t.Fatal("saving settings rewrote retained developer preference fields")
		}
	})
	t.Run("order survives a restart", func(t *testing.T) {
		path := filepath.Join(t.TempDir(), "preferences.json")
		start := func() *Runtime {
			r := &Runtime{ctx: t.Context(), App: shell.NewLive(Sidebar(snapshot, preferences.Default())), Preferences: preferences.Open(path)}
			r.applyPreferences()
			return r
		}
		r := start()
		if r.Preferences.Data.SessionOrder != nil || r.App.Settings.Model.SessionOrder != int(sidebar.SortUpdated) || *r.App.Sidebar.Sort != int(sidebar.SortUpdated) {
			t.Fatalf("a new profile starts in order %d", r.App.Settings.Model.SessionOrder)
		}
		// Unchanged, the default writes no choice.
		r.savePreferences()
		r.workers.Wait()
		if r.Preferences.Data.SessionOrder != nil {
			t.Fatalf("the default order was written as %q", *r.Preferences.Data.SessionOrder)
		}
		for _, by := range []sidebar.Sort{sidebar.SortName, sidebar.SortCreated} {
			// The sort menu sets the sidebar's order, which is the setting.
			*r.App.Sidebar.Sort = int(by)
			r.savePreferences()
			r.workers.Wait()
			if stored := preferences.Open(path).Data.SessionOrder; stored == nil || *stored != by.Preference() {
				t.Fatalf("order %q was stored as %v", by.Preference(), stored)
			}
			r = start()
			if r.App.Settings.Model.SessionOrder != int(by) || *r.App.Sidebar.Sort != int(by) {
				t.Fatalf("order %q came back as %d", by.Preference(), r.App.Settings.Model.SessionOrder)
			}
			// After the chat, atlas leads by name and as the newest project;
			// by the default, Workspace would.
			if got := r.App.Sidebar.Order(); got[1] != name(ids[7]) {
				t.Fatalf("order %q after the restart lists %v", by.Preference(), got)
			}
		}
	})
	if snapshot.Sessions[0].ID != ids[0] || prefs.Pin(node, ids[0].String()).Order == nil || *prefs.Pin(node, ids[0].String()).Order != math.MaxUint64 {
		t.Fatal("presentation changed authority order or full-width preference")
	}
}

func TestNoticesProjection(t *testing.T) {
	// The viewer draws with a ui.Tester, whose font atlas every tester
	// shares: it runs before this test joins the parallel ones.
	t.Run("knowledge viewer", testKnowledgeViewer)
	t.Parallel()
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

// A manual move names every session of the project in the Node's order,
// the delegated and archived ones the sidebar does not list included, and
// only the project's.
func testSessionOrderCommand(t *testing.T) {
	project, other := protocol.NewProjectID(), protocol.NewProjectID()
	var sessions []protocol.Session
	for i := range 5 {
		session := protocol.Session{ID: protocol.NewSessionID(), Project: &project}
		switch i {
		case 1:
			session.Archived = true
		case 3:
			session.Project = &other
		case 4:
			session.Project = nil
		}
		sessions = append(sessions, session)
	}
	id := func(i int) string { return sessions[i].ID.String() }
	command, ok := sessionOrderCommand(sessions, id(0), id(2), true)
	if !ok || command.Kind != "set_session_order" {
		t.Fatalf("command = %#v, %v", command, ok)
	}
	payload := command.Data.(protocol.CommandSetSessionOrderPayload)
	expected := []protocol.SessionID{sessions[0].ID, sessions[1].ID, sessions[2].ID}
	order := []protocol.SessionID{sessions[1].ID, sessions[2].ID, sessions[0].ID}
	if payload.Project != project || !slices.Equal(payload.Expected, expected) || !slices.Equal(payload.Sessions, order) {
		t.Fatalf("payload = %#v", payload)
	}
	for _, move := range [][2]int{{0, 3}, {0, 4}, {4, 0}, {0, 0}, {0, 1}} {
		after := move == [2]int{0, 1}
		if _, ok := sessionOrderCommand(sessions, id(move[0]), id(move[1]), !after); ok {
			t.Errorf("move %v made a command", move)
		}
	}
}
