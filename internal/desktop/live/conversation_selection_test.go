package live

import (
	"context"
	"path/filepath"
	"slices"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/panes"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type selectionTestTransport struct {
	conversationTestTransport
	node *conversationTestSubscription
}

func (s *selectionTestTransport) Subscribe(ctx context.Context, topic protocol.Topic) (link.Subscription, error) {
	switch topic.Kind {
	case "node":
		return s.node, nil
	case "conversation":
		snapshot := protocol.DefaultConversationSnapshot()
		snapshot.Node, snapshot.Page.Session = s.Target(), topic.Data.(protocol.SessionID)
		stream := &conversationTestSubscription{updates: make(chan protocol.Update, 1), closed: make(chan struct{})}
		stream.updates <- protocol.Update{Kind: "conversation_snapshot", Data: snapshot}
		return stream, nil
	default:
		return s.conversationTestTransport.Subscribe(ctx, topic)
	}
}

func selectionSnapshot(titles ...string) protocol.Snapshot {
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node = protocol.NodeID{1}
	for i, title := range titles {
		session := protocol.DefaultSession()
		session.ID, session.Revision, session.Activity.Title = protocol.SessionID{byte(i + 1)}, 1, title
		snapshot.Sessions = append(snapshot.Sessions, session)
	}
	return snapshot
}

type selectionTestWindow struct {
	r       *Runtime
	window  *ui.Tester
	source  *selectionTestTransport
	changes chan uint64
}

func selectionWindow(t *testing.T, snapshot protocol.Snapshot) selectionTestWindow {
	t.Helper()
	ctx, cancel := context.WithTimeout(t.Context(), 10*time.Second)
	t.Cleanup(cancel)
	source := &selectionTestTransport{
		conversationTestTransport: conversationTestTransport{snapshot: snapshot},
		node:                      &conversationTestSubscription{updates: make(chan protocol.Update, 1), closed: make(chan struct{})},
	}
	source.node.updates <- protocol.Update{Kind: "snapshot", Data: snapshot}
	source.handle = func(request protocol.Request) (link.Response, bool) {
		if request.Command.Kind != "remove_session" {
			return link.Response{}, false
		}
		input := request.Command.Data.(protocol.CommandRemoveSessionPayload)
		return link.Response{Output: protocol.Output{Kind: "sessions_removed", Data: []protocol.SessionID{input.Session}}}, true
	}
	controller := client.New(source)
	t.Cleanup(func() { _ = controller.Close() })
	if err := controller.Start(ctx); err != nil {
		t.Fatal(err)
	}
	prefs := preferences.Open(filepath.Join(t.TempDir(), "desktop", "preferences.json"))
	prefs.Data.WelcomeSeen = new(true)
	r, err := NewWithPreferences(ctx, controller, prefs)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = r.Close() })
	t.Cleanup(func() {
		r.workers.Wait()
		source.mu.Lock()
		defer source.mu.Unlock()
		for _, request := range source.requests {
			switch request.Command.Kind {
			case "create_session", "create_session_at", "start_session":
				t.Errorf("selecting a fallback started a Node session: %s", request.Command.Kind)
			}
		}
	})
	changes := make(chan uint64, 8)
	r.unsubscribe()
	r.unsubscribe = controller.OnChange(func() {
		r.changed()
		select {
		case changes <- controller.Snapshot().Cursor:
		default:
		}
	})
	window := ui.NewTesterWith(r.App.View, 1280, 820, ui.TesterOptions{Preferences: ui.Preferences{ReduceMotion: true, ReduceTransparency: true}})
	window.SetLocale("en")
	window.Frame()
	return selectionTestWindow{r: r, window: window, source: source, changes: changes}
}

func (f selectionTestWindow) update(t *testing.T, cursor uint64, update protocol.Update) {
	t.Helper()
	f.source.node.updates <- update
	for {
		select {
		case got := <-f.changes:
			if got == cursor {
				f.window.Frame()
				return
			}
		case <-f.r.ctx.Done():
			t.Fatalf("Client did not apply %s at cursor %d: %v", update.Kind, cursor, f.r.controller.Err())
		}
	}
}

func (f selectionTestWindow) event(t *testing.T, kind string, data any) {
	t.Helper()
	cursor := f.r.controller.Snapshot().Cursor + 1
	f.update(t, cursor, protocol.Update{Kind: "event", Data: protocol.EventEnvelope{
		Node: f.source.Target(), Cursor: cursor, Event: protocol.Event{Kind: kind, Data: data},
	}})
}

func (f selectionTestWindow) composer(t *testing.T) ui.AccessInfo {
	t.Helper()
	field, ok := f.window.AccessByRole(ui.RoleTextField, i18n.In("en", "composer"))
	if !ok || field.Disabled || field.Bounds.W <= 0 || field.Bounds.H <= 0 {
		t.Fatalf("no usable composer on page %v: %q", f.r.App.Page, f.window.Texts())
	}
	return field
}

func TestSessionDeletionReturnsToDraft(t *testing.T) {
	snapshot := selectionSnapshot("Current chat", "Next chat", "Embedded assistant")
	first, second, assistant := snapshot.Sessions[0].ID, snapshot.Sessions[1].ID, snapshot.Sessions[2].ID
	snapshot.Sessions[2].Config.Assistant = &protocol.PluginConversationBinding{Package: protocol.PluginReference{Name: "fixture"}, ID: "assistant"}
	f := selectionWindow(t, snapshot)
	r, app := f.r, f.r.App
	r.OpenSession(first.String())
	f.window.Frame()
	for _, id := range []protocol.SessionID{first, second} {
		cv := app.Data.Find(id.String())
		if cv == nil || app.FocusedConversation() != cv.ID {
			t.Fatalf("session %s is not the current fallback: %s", id, app.FocusedConversation())
		}
		r.removeSession(cv)
		r.workers.Wait()
		f.window.Frame()
		if app.FocusedConversation() != cv.ID || r.drafts != 0 {
			t.Fatal("delete acknowledgement changed the view before the Client observed removal")
		}
		f.event(t, "sessions_removed", []protocol.SessionID{id})
		if app.Data.Find(id.String()) != nil {
			t.Fatalf("removed session %s remained in Desktop", id)
		}
	}
	f.source.mu.Lock()
	var removals []protocol.CommandRemoveSessionPayload
	for _, request := range f.source.requests {
		if request.Command.Kind == "remove_session" {
			removals = append(removals, request.Command.Data.(protocol.CommandRemoveSessionPayload))
		}
	}
	f.source.mu.Unlock()
	if !slices.Equal(removals, []protocol.CommandRemoveSessionPayload{{Session: first, ExpectedRevision: 1}, {Session: second, ExpectedRevision: 1}}) {
		t.Fatalf("Desktop did not send the expected removals: %+v", removals)
	}
	draft := app.FocusedConversation()
	if cv := app.Data.Find(draft); cv == nil || !unsentDraft(cv) || len(app.Sidebar.Order()) != 0 || app.Data.Find(assistant.String()) == nil {
		t.Fatalf("selected %q instead of a draft with only the embedded assistant left", draft)
	}
	field := f.composer(t)
	f.window.ClickAt(field.Bounds.X+field.Bounds.W/2, field.Bounds.Y+field.Bounds.H/2)
	f.window.Type("Local input")
	f.window.Frame()
	if got := app.Composer(draft).Draft; got != "Local input" {
		t.Fatalf("typing did not reach the fallback composer: %q", got)
	}
	f.event(t, "cursor", nil)
	if got := app.Composer(draft).Draft; app.FocusedConversation() != draft || got != "Local input" || r.drafts != 1 {
		t.Fatalf("unrelated update changed the local draft %q: input %q, drafts %d", draft, got, r.drafts)
	}
}

func TestEmptySessionSnapshotReusesDraft(t *testing.T) {
	for _, test := range []struct {
		name string
		page shell.Page
	}{{"conversation", shell.ConversationPage}, {"independent resource", shell.GitPage}} {
		t.Run(test.name, func(t *testing.T) {
			snapshot := selectionSnapshot("Prior chat")
			project := protocol.Project{ID: protocol.ProjectID{1}, Name: "Independent project"}
			snapshot.Projects = []protocol.Project{project}
			f := selectionWindow(t, snapshot)
			r, app := f.r, f.r.App
			r.newDraft(nil)
			draft := app.Sidebar.Selected
			composer := app.Composer(draft)
			composer.Draft = "Keep this draft"
			r.OpenSession(snapshot.Sessions[0].ID.String())
			f.window.Frame()
			target := shell.ResourceTarget{Node: snapshot.Node.String(), Project: project.ID.String()}
			if test.page == shell.GitPage {
				app.SetResourceTarget(target)
				app.Navigate(test.page)
				f.window.Frame()
			}
			snapshot.Cursor++
			snapshot.Sessions = nil
			f.update(t, snapshot.Cursor, protocol.Update{Kind: "snapshot", Data: snapshot})
			if len(r.controller.Snapshot().Sessions) != 0 || app.Page != test.page || r.drafts != 1 {
				t.Fatalf("empty snapshot changed page %v to %v or replaced the draft: %d drafts", test.page, app.Page, r.drafts)
			}
			if test.page == shell.GitPage {
				if app.ResourceTarget() != target || app.Sidebar.Selected == draft {
					t.Fatal("empty snapshot changed the resource target or selected the background draft")
				}
				app.Navigate(shell.ConversationPage)
				f.window.Frame()
			}
			if app.FocusedConversation() != draft || app.Composer(draft) != composer || f.composer(t).Value != "Keep this draft" {
				t.Fatalf("the conversation page did not restore editable draft %q: selected %q", draft, app.FocusedConversation())
			}
		})
	}
}

func selectionSplit(targets ...panes.Target) *panes.Node {
	group := &panes.Node{Axis: panes.Row}
	for _, t := range targets {
		group.Children = append(group.Children, &panes.Node{Target: t})
		group.Sizes = append(group.Sizes, 1)
	}
	return group
}

func TestSessionDeletionKeepsSurvivingSplit(t *testing.T) {
	f := selectionWindow(t, selectionSnapshot("One", "Two", "Three", "Four", "Five"))
	r, app := f.r, f.r.App
	ids := app.Sidebar.Order()
	session := func(i int) panes.Target { return panes.Target{Kind: panes.Session, ID: ids[i]} }
	current, background, sibling := session(1), session(2), session(4)
	other := selectionSplit(session(0), session(3))
	other.Sizes = []float32{0.375, 0.625}
	prefs := app.Prefs()
	prefs.Panes = []*panes.Node{selectionSplit(current, background, sibling), other}
	app.SetPrefs(prefs)
	r.OpenSession(current.ID)
	app.Composer(sibling.ID).Draft = "Split input"
	f.window.Frame()

	backgroundID, _ := protocol.ParseSessionID(background.ID)
	f.event(t, "sessions_removed", []protocol.SessionID{backgroundID})
	if groups := app.Prefs().Panes; app.FocusedConversation() != current.ID || len(groups) != 2 || !slices.Equal(groups[0].Targets(), []panes.Target{current, sibling}) {
		t.Fatalf("removing a background pane changed the focused split: %q, %v", app.FocusedConversation(), groups)
	}
	currentID, _ := protocol.ParseSessionID(current.ID)
	f.event(t, "sessions_removed", []protocol.SessionID{currentID})
	groups := app.Prefs().Panes
	if app.FocusedConversation() != sibling.ID || len(groups) != 1 || r.drafts != 0 ||
		!slices.Equal(groups[0].Targets(), []panes.Target{session(0), session(3)}) || !slices.Equal(groups[0].Sizes, []float32{0.375, 0.625}) {
		t.Fatalf("removing the focused pane chose %q or changed unrelated splits: %v", app.FocusedConversation(), groups)
	}
	if got := f.composer(t).Value; got != "Split input" {
		t.Fatalf("surviving pane lost its composer: %q", got)
	}
}

// Only the Node's removal, its event or a delete's answer, forgets a session's
// tab and its place in a saved split. A snapshot that stops listing a watched
// session, as when its permission is withdrawn, keeps both, and they return
// with it; a removal another client made of a session not shown forgets it;
// and a session ID another host still lists keeps its tab.
func TestSessionTabsForgottenOnlyOnRemoval(t *testing.T) {
	snapshot := selectionSnapshot("Deleted here", "Withdrawn", "Deleted elsewhere", "Shared")
	deleted, withdrawn, background, shared := snapshot.Sessions[0].ID, snapshot.Sessions[1].ID, snapshot.Sessions[2].ID, snapshot.Sessions[3].ID
	f := selectionWindow(t, snapshot)
	r, app := f.r, f.r.App
	target := func(id protocol.SessionID) panes.Target { return panes.Target{Kind: panes.Session, ID: id.String()} }
	saved := func() []panes.Target {
		var out []panes.Target
		for _, group := range app.Prefs().Panes {
			out = append(out, group.Targets()...)
		}
		return out
	}
	remote := protocol.DefaultSnapshot()
	remote.Node = protocol.NodeID{2}
	listed := protocol.DefaultSession()
	listed.ID = shared
	remote.Sessions = []protocol.Session{listed}
	bound := r.newBinding(ownershipClient(t, remote))
	t.Cleanup(func() { _ = bound.Close() })
	r.bindings = map[string]*Runtime{remote.Node.String(): bound}
	prefs := app.Prefs()
	prefs.Tabs = []panes.Target{target(deleted), target(shared)}
	prefs.Panes = []*panes.Node{selectionSplit(target(withdrawn), target(background))}
	app.SetPrefs(prefs)
	r.OpenSession(withdrawn.String())
	f.window.Frame()
	wantSplit := []panes.Target{target(withdrawn), target(background)}
	if got := saved(); !slices.Equal(got, wantSplit) || !r.conversationSeen[withdrawn] || !r.conversationSeen[background] {
		t.Fatalf("the initial split is not displayed: panes %v, shown %v", got, r.conversationSeen)
	}

	snapshotAt := func(sessions []protocol.Session) {
		t.Helper()
		next := snapshot
		next.Sessions = sessions
		next.Cursor = r.controller.Snapshot().Cursor + 1
		f.update(t, next.Cursor, protocol.Update{Kind: "snapshot", Data: next})
		f.window.Frame()
	}
	snapshotAt(slices.DeleteFunc(slices.Clone(snapshot.Sessions), func(s protocol.Session) bool { return s.ID == withdrawn }))
	if app.Data.Find(withdrawn.String()) != nil || r.conversationSeen[withdrawn] {
		t.Fatal("the withdrawn session is still visible")
	}
	// Both hosts list the shared ID; the sidebar draws a row for each.
	var hosts []string
	for _, cv := range app.Data.Unassigned {
		if cv.ID == shared.String() {
			hosts = append(hosts, cv.Node)
		}
	}
	if len(hosts) != 2 || hosts[0] == hosts[1] {
		t.Fatalf("the shared ID is not listed once per host: %q", hosts)
	}
	if !slices.Contains(saved(), target(withdrawn)) || !slices.Contains(app.Prefs().Tabs, target(deleted)) {
		t.Fatalf("a withdrawn session lost its place: panes %v, tabs %v", saved(), app.Prefs().Tabs)
	}
	snapshotAt(slices.Clone(snapshot.Sessions))
	if got := saved(); !slices.Equal(got, wantSplit) || !r.conversationSeen[withdrawn] || !r.conversationSeen[background] {
		t.Fatalf("a restored session did not return to its split: panes %v, shown %v", got, r.conversationSeen)
	}

	f.event(t, "sessions_removed", []protocol.SessionID{background, shared})
	if got := saved(); slices.Contains(got, target(background)) {
		t.Fatalf("a session another client removed kept its split place: %v", got)
	}
	if !slices.Contains(app.Prefs().Tabs, target(shared)) {
		t.Fatalf("a session another host lists lost its tab: %v", app.Prefs().Tabs)
	}

	r.removeSession(app.Data.Find(deleted.String()))
	r.workers.Wait()
	f.window.Frame()
	f.window.Frame()
	if slices.Contains(app.Prefs().Tabs, target(deleted)) {
		t.Fatalf("a confirmed delete kept its tab: %v", app.Prefs().Tabs)
	}
}

func TestSessionDeletionKeepsWorkspaceTerminal(t *testing.T) {
	snapshot := selectionSnapshot("Split chat", "Other chat")
	inside, outside := snapshot.Sessions[0].ID, snapshot.Sessions[1].ID
	terminal := protocol.TerminalID{1}
	snapshot.Terminals = []protocol.TerminalInfo{{ID: terminal, Status: protocol.TerminalStatus{Kind: "running"}}}
	f := selectionWindow(t, snapshot)
	r, app := f.r, f.r.App
	const screen = "Independent terminal screen"
	app.Panel.TerminalActions.Pane = func(c *ui.Context, _ string) { ui.Text(c, screen) }
	prefs := app.Prefs()
	prefs.WorkspaceTerminals = []shell.WorkspaceTerminal{{ID: terminal.String(), Title: "Workspace shell"}}
	prefs.Panes = []*panes.Node{selectionSplit(panes.Target{Kind: panes.Session, ID: inside.String()}, panes.Target{Kind: panes.Terminal, ID: terminal.String()})}
	app.SetPrefs(prefs)
	r.OpenSession(outside.String())
	app.OpenWorkspaceTerminal(terminal.String(), "Workspace shell")
	f.window.Frame()
	f.event(t, "sessions_removed", []protocol.SessionID{outside})
	if app.FocusedConversation() != "" || !f.window.HasText(screen) || r.drafts != 0 {
		t.Fatal("deleting a session in the background displaced the focused terminal")
	}
	r.OpenSession(inside.String())
	f.window.Frame()
	f.event(t, "sessions_removed", []protocol.SessionID{inside})
	if app.FocusedConversation() != "" || !f.window.HasText(screen) || r.drafts != 0 {
		t.Fatal("deleting the last session displaced a surviving split terminal")
	}
	app.DropWorkspaceTerminal(terminal.String())
	f.window.Frame()
	if cv := app.Data.Find(app.FocusedConversation()); cv == nil || !unsentDraft(cv) {
		t.Fatal("leaving the terminal did not recover the empty conversation view")
	}
	f.composer(t)
}
