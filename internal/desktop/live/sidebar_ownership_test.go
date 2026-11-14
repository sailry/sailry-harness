package live

import (
	"path/filepath"
	"slices"
	"testing"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func ownershipClient(t *testing.T, snapshot protocol.Snapshot) *client.Client {
	t.Helper()
	transport := &conversationTestTransport{snapshot: snapshot, conversation: protocol.DefaultConversationSnapshot()}
	controller := client.New(transport)
	if err := controller.Start(t.Context()); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = controller.Close() })
	return controller
}

func TestSidebarKeepsResourceSessionsInTheirSurface(t *testing.T) {
	project := protocol.ProjectID{1}
	assistant := &protocol.PluginConversationBinding{Package: protocol.PluginReference{Name: "fixture"}, ID: "assistant"}
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node = protocol.NodeID{1}
	snapshot.Projects = []protocol.Project{{ID: project, Name: "Project"}}
	snapshot.Sessions = []protocol.Session{
		{ID: protocol.SessionID{1}},
		{ID: protocol.SessionID{2}, Config: protocol.SessionConfig{Assistant: assistant}},
		{ID: protocol.SessionID{3}, Config: protocol.SessionConfig{Resource: &protocol.ConnectionResource{Kind: "ssh", Data: protocol.SshID{1}}}},
		{ID: protocol.SessionID{4}, Config: protocol.SessionConfig{Assistant: assistant, Resource: &protocol.ConnectionResource{Kind: "database", Data: protocol.DatabaseID{1}}}},
		{ID: protocol.SessionID{5}, Project: &project, Config: protocol.SessionConfig{Assistant: assistant}},
		{ID: protocol.SessionID{6}, Project: &project},
		{ID: protocol.SessionID{7}, Archived: true, Config: protocol.SessionConfig{Assistant: assistant}},
	}
	prefs := preferences.Default()
	prefs.SetPin(snapshot.Node, snapshot.Sessions[1].ID.String(), preferences.Pin{Pinned: true})
	r := &Runtime{controller: ownershipClient(t, snapshot)}
	admitted := protocol.Session{ID: protocol.SessionID{8}, Config: protocol.SessionConfig{Assistant: assistant}}
	r.admittedSessions = map[string]protocol.Session{admitted.ID.String(): admitted}
	projected := snapshot
	r.projectAdmission(&projected)
	data := Sidebar(projected, prefs)
	draft := &viewmodel.Conversation{Node: snapshot.Node.String(), ID: "draft/ordinary"}
	data.Unassigned = append(data.Unassigned, draft)
	remote := protocol.DefaultSnapshot()
	remote.Node = protocol.NodeID{2}
	remote.Sessions = []protocol.Session{
		{ID: protocol.SessionID{9}, Config: protocol.SessionConfig{Assistant: assistant}},
		{ID: protocol.SessionID{10}},
	}
	bound := &Runtime{controller: ownershipClient(t, remote), parent: r}
	r.bindings = map[string]*Runtime{remote.Node.String(): bound}
	data.Unassigned = append(data.Unassigned, Sidebar(remote, prefs).Unassigned...)
	r.App = shell.NewLive(data)
	r.listSidebar()

	want := []string{snapshot.Sessions[0].ID.String(), remote.Sessions[1].ID.String(), snapshot.Sessions[5].ID.String()}
	if got := r.App.Sidebar.Order(); !slices.Equal(got, want) {
		t.Fatalf("ordinary navigation contains resource sessions: %v, want %v", got, want)
	}
	for _, id := range []protocol.SessionID{{2}, {3}, {4}, {5}, {7}, {8}, {9}} {
		if r.App.Sidebar.Data.Find(id.String()) != nil || r.App.Data.Find(id.String()) == nil {
			t.Fatalf("resource session %s leaked into navigation or lost its embedded view", id)
		}
	}
	if r.App.Data.ProjectOf(snapshot.Sessions[4].ID.String()) == nil || r.App.Data.Find(draft.ID) != draft || r.App.Sidebar.Data.Find(draft.ID) != nil {
		t.Fatal("filtering lost workspace ownership or changed draft visibility")
	}
	if !r.App.Data.Find(snapshot.Sessions[1].ID.String()).Pinned || len(snapshot.Sessions) != 7 {
		t.Fatal("navigation filtering mutated persisted pin state or its source snapshot")
	}
}

func TestDefaultSessionSelectionExcludesAssistants(t *testing.T) {
	for _, ordinary := range []bool{false, true} {
		t.Run(map[bool]string{false: "assistant only", true: "ordinary available"}[ordinary], func(t *testing.T) {
			snapshot := protocol.DefaultSnapshot()
			snapshot.Node = protocol.NodeID{1}
			assistant := protocol.DefaultSession()
			assistant.ID = protocol.SessionID{1}
			assistant.Config.Assistant = &protocol.PluginConversationBinding{Package: protocol.PluginReference{Name: "fixture"}, ID: "assistant"}
			snapshot.Sessions = []protocol.Session{assistant}
			chat := protocol.DefaultSession()
			chat.ID = protocol.SessionID{2}
			if ordinary {
				snapshot.Sessions = append(snapshot.Sessions, chat)
			}
			r, err := NewWithPreferences(t.Context(), ownershipClient(t, snapshot), preferences.Open(filepath.Join(t.TempDir(), "preferences.json")))
			if err != nil {
				t.Fatal(err)
			}
			defer r.Close()
			if r.App.Sidebar.Selected == assistant.ID.String() || ordinary && r.App.Sidebar.Selected != chat.ID.String() {
				t.Fatalf("startup chose %q", r.App.Sidebar.Selected)
			}
			// ApplyData's fallback must use the filtered list after the selected
			// conversation disappears, even when an assistant is first.
			r.App.Sidebar.Selected = "removed-session"
			r.project()
			if r.App.Sidebar.Selected == assistant.ID.String() || ordinary && r.App.Sidebar.Selected != chat.ID.String() {
				t.Fatalf("snapshot refresh chose %q", r.App.Sidebar.Selected)
			}
			r.newDraft(nil)
			draft := r.App.Sidebar.Selected
			r.project()
			if r.App.Sidebar.Selected != draft {
				t.Fatal("snapshot refresh discarded the selected draft")
			}
			r.removeDraft(draft)
			if r.App.Sidebar.Selected == assistant.ID.String() || ordinary && r.App.Sidebar.Selected != chat.ID.String() {
				t.Fatalf("removing a draft chose %q", r.App.Sidebar.Selected)
			}
			// An explicit notification/session link can still open the transcript.
			r.OpenSession(assistant.ID.String())
			r.project()
			if r.App.Sidebar.Selected != assistant.ID.String() || r.App.Sidebar.Data.Find(assistant.ID.String()) != nil {
				t.Fatal("explicit navigation lost its transcript or leaked it into the sidebar")
			}
		})
	}
}
