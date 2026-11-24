package live

import (
	"context"
	"slices"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/panes"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestSessionRemovalInspectsOriginalRequestAcrossRevisionChange(t *testing.T) {
	w := selectionWindow(t, selectionSnapshot("Keep on failure"))
	r := w.r
	id := w.source.snapshot.Sessions[0].ID
	row := &viewmodel.Conversation{ID: id.String(), Node: r.controller.Target().String(), Revision: 1}
	target := panes.Target{Kind: panes.Session, ID: row.ID}
	prefs := r.App.Prefs()
	prefs.Tabs = []panes.Target{target}
	r.App.SetPrefs(prefs)
	var removals, inspections int
	var original protocol.RequestID
	w.source.mu.Lock()
	w.source.handle = func(req protocol.Request) (link.Response, bool) {
		switch req.Command.Kind {
		case "remove_session":
			removals++
			original = req.ID
			return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown}}, true
		case "inspect_request":
			inspections++
			query := req.Command.Data.(protocol.CommandInspectRequestPayload)
			if query.ID != original {
				t.Errorf("inspected %s, want %s", query.ID, original)
			}
			outcome := protocol.RequestOutcome{Kind: "admitted"}
			if inspections > 1 {
				outcome = protocol.RequestOutcome{Kind: "completed", Data: protocol.Result[protocol.Output, protocol.Fault]{Err: &protocol.Fault{Code: protocol.ErrorCodeRevisionConflict}}}
			}
			return link.Response{Output: protocol.Output{Kind: "request_outcome", Data: protocol.OutputRequestOutcomePayload{ID: query.ID, Outcome: outcome}}}, true
		}
		return link.Response{}, false
	}
	w.source.mu.Unlock()
	remove := func() { r.removeSession(row); r.workers.Wait(); w.window.Frame() }
	remove()
	if len(r.sessionRemovals) != 1 {
		t.Fatal("uncertain removal lost its original request")
	}
	row.Revision++
	remove()
	if removals != 1 || inspections != 2 || len(r.sessionRemovals) != 0 {
		t.Fatalf("removals %d, inspections %d, outstanding %d", removals, inspections, len(r.sessionRemovals))
	}
	if tabs := r.App.Prefs().Tabs; len(tabs) != 1 || tabs[0] != target || r.App.Data.Find(row.ID) == nil {
		t.Fatalf("failed removal discarded the session or its tab: %v", tabs)
	}
}

func TestSelectedSessionRemovalBatchesByNode(t *testing.T) {
	local := selectionSnapshot("Shared ID", "Local first", "Local second")
	local.Sessions[1].Revision = 5
	w := selectionWindow(t, local)
	r := w.r
	remote := selectionSnapshot("Remote first", "Remote second", "Remote child")
	remote.Node = protocol.NodeID{2}
	remote.Sessions[0].Revision = 7
	remote.Sessions[1].ID, remote.Sessions[1].Revision = protocol.SessionID{8}, 9
	remote.Sessions[2].ID = protocol.SessionID{9}
	child := remote.Sessions[2].ID
	peer := &selectionTestTransport{
		conversationTestTransport: conversationTestTransport{snapshot: remote},
		node:                      &conversationTestSubscription{updates: make(chan protocol.Update, 1), closed: make(chan struct{})},
	}
	peer.handle = func(req protocol.Request) (link.Response, bool) {
		if req.Command.Kind != "remove_sessions" {
			return link.Response{}, false
		}
		return link.Response{Output: protocol.Output{Kind: "sessions_removed", Data: []protocol.SessionID{remote.Sessions[0].ID, remote.Sessions[1].ID, child}}}, true
	}
	w.source.mu.Lock()
	w.source.handle = func(req protocol.Request) (link.Response, bool) {
		if req.Command.Kind != "remove_sessions" {
			return link.Response{}, false
		}
		return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeRevisionConflict}}, true
	}
	w.source.mu.Unlock()
	peer.node.updates <- protocol.Update{Kind: "snapshot", Data: remote}
	controller := client.New(peer)
	if err := controller.Start(r.ctx); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = controller.Close() })
	r.bindings = map[string]*Runtime{remote.Node.String(): r.newBinding(controller)}
	r.changed()
	w.window.Frame()

	draft := r.createDraftIn(nil)
	draft.Node = protocol.NodeID{3}.String()
	var kept []panes.Target
	for _, row := range local.Sessions {
		kept = append(kept, panes.Target{Kind: panes.Session, ID: row.ID.String()})
	}
	childTab := panes.Target{Kind: panes.Session, ID: child.String()}
	prefs := r.App.Prefs()
	prefs.Tabs = append(slices.Clone(kept), childTab)
	r.App.SetPrefs(prefs)
	if !slices.Contains(r.App.Prefs().Tabs, childTab) {
		t.Fatal("the remote child did not have a tab")
	}

	localExpected := []protocol.SessionRevision{
		{Session: local.Sessions[1].ID, Revision: 4},
		{Session: local.Sessions[2].ID, Revision: local.Sessions[2].Revision},
	}
	remoteExpected := []protocol.SessionRevision{
		{Session: remote.Sessions[0].ID, Revision: remote.Sessions[0].Revision},
		{Session: remote.Sessions[1].ID, Revision: remote.Sessions[1].Revision},
	}
	row := func(node protocol.NodeID, entry protocol.SessionRevision) *viewmodel.Conversation {
		return &viewmodel.Conversation{Node: node.String(), ID: entry.Session.String(), Revision: entry.Revision}
	}
	r.App.Sidebar.Actions.Delete([]*viewmodel.Conversation{
		row(local.Node, localExpected[0]), row(remote.Node, remoteExpected[0]),
		{ID: draft.ID, Node: draft.Node}, row(local.Node, localExpected[1]), row(remote.Node, remoteExpected[1]),
	})
	r.workers.Wait()
	w.window.Frame()

	checkBatch := func(source *selectionTestTransport, node protocol.NodeID, want []protocol.SessionRevision) {
		t.Helper()
		source.mu.Lock()
		defer source.mu.Unlock()
		var batches int
		for _, req := range source.requests {
			switch req.Command.Kind {
			case "remove_session":
				t.Errorf("sent an individual removal to %s", req.Target)
			case "remove_sessions":
				batches++
				got := req.Command.Data.(protocol.CommandRemoveSessionsPayload).Expected
				if req.Target != node || !slices.Equal(got, want) {
					t.Errorf("submitted %v on %s, want %v on %s", got, req.Target, want, node)
				}
			}
		}
		if batches != 1 {
			t.Errorf("submitted %d batches on %s, want 1", batches, node)
		}
	}
	checkBatch(w.source, local.Node, localExpected)
	checkBatch(peer, remote.Node, remoteExpected)
	if r.App.Data.Find(draft.ID) != nil {
		t.Fatal("the draft was not removed locally")
	}
	tabs := r.App.Prefs().Tabs
	for _, tab := range kept {
		if !slices.Contains(tabs, tab) {
			t.Errorf("lost the tab for surviving local session %s", tab.ID)
		}
	}
	if slices.Contains(tabs, childTab) {
		t.Fatal("the remote cascade left a tab for a removed session")
	}
}

// The Node stops running sessions in a removal before removing them, so the
// Desktop sends the removal alone rather than its own stops.
func TestRunningSessionRemovalLeavesStopsToNode(t *testing.T) {
	snapshot := selectionSnapshot("Running", "Finished")
	w := selectionWindow(t, snapshot)
	r := w.r
	ids := []protocol.SessionID{snapshot.Sessions[0].ID, snapshot.Sessions[1].ID}
	w.source.mu.Lock()
	w.source.handle = func(req protocol.Request) (link.Response, bool) {
		if req.Command.Kind != "remove_sessions" {
			return link.Response{}, false
		}
		return link.Response{Output: protocol.Output{Kind: "sessions_removed", Data: ids}}, true
	}
	w.source.mu.Unlock()
	r.App.Sidebar.Actions.Delete([]*viewmodel.Conversation{
		{ID: ids[0].String(), Node: snapshot.Node.String(), Revision: 1, Status: viewmodel.Running},
		{ID: ids[1].String(), Node: snapshot.Node.String(), Revision: 1},
	})
	r.workers.Wait()
	w.window.Frame()
	w.source.mu.Lock()
	defer w.source.mu.Unlock()
	var removals int
	for _, req := range w.source.requests {
		switch req.Command.Kind {
		case "stop_turn":
			t.Fatal("Desktop stopped a turn the Node's removal stops")
		case "remove_sessions":
			removals++
			want := []protocol.SessionRevision{{Session: ids[0], Revision: 1}, {Session: ids[1], Revision: 1}}
			if got := req.Command.Data.(protocol.CommandRemoveSessionsPayload).Expected; !slices.Equal(got, want) {
				t.Fatalf("removal sent %v, want %v", got, want)
			}
		}
	}
	if removals != 1 || len(r.sessionRemovals) != 0 {
		t.Fatalf("sent %d removals with %d outstanding", removals, len(r.sessionRemovals))
	}
	for _, id := range ids {
		if !r.sessionHidden(id) {
			t.Fatalf("removed session %s is still shown", id)
		}
	}
}

func TestSelectedSessionRemovalRecoversOnlyMatchingTargets(t *testing.T) {
	snapshot := selectionSnapshot("First", "Second", "Third")
	w := selectionWindow(t, snapshot)
	r := w.r
	var removals, inspections []protocol.Request
	w.source.mu.Lock()
	w.source.handle = func(req protocol.Request) (link.Response, bool) {
		switch req.Command.Kind {
		case "remove_sessions":
			removals = append(removals, req)
			if len(removals) == 1 {
				return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown}}, true
			}
			return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeRevisionConflict}}, true
		case "inspect_request":
			inspections = append(inspections, req)
			query := req.Command.Data.(protocol.CommandInspectRequestPayload)
			outcome := protocol.RequestOutcome{Kind: "admitted"}
			if len(inspections) > 1 {
				outcome = protocol.RequestOutcome{Kind: "completed", Data: protocol.Result[protocol.Output, protocol.Fault]{Err: &protocol.Fault{Code: protocol.ErrorCodeRevisionConflict}}}
			}
			return link.Response{Output: protocol.Output{Kind: "request_outcome", Data: protocol.OutputRequestOutcomePayload{ID: query.ID, Outcome: outcome}}}, true
		}
		return link.Response{}, false
	}
	w.source.mu.Unlock()
	var rows []*viewmodel.Conversation
	for _, session := range snapshot.Sessions {
		rows = append(rows, &viewmodel.Conversation{Node: snapshot.Node.String(), ID: session.ID.String(), Revision: session.Revision})
	}
	remove := func(rows ...*viewmodel.Conversation) {
		r.App.Sidebar.Actions.Delete(rows)
		r.workers.Wait()
		w.window.Frame()
	}
	remove(rows[0], rows[1])
	if len(removals) != 1 || len(inspections) != 1 || len(r.sessionRemovals) != 1 {
		t.Fatalf("uncertain batch: %d removals, %d inspections, %d pending", len(removals), len(inspections), len(r.sessionRemovals))
	}
	remove(rows[0], rows[2])
	if len(removals) != 1 || len(inspections) != 1 || len(r.sessionRemovals) != 1 {
		t.Fatal("a different selection ran or inspected the unresolved batch")
	}
	rows[0].Revision++
	rows[1].Revision++
	remove(rows[1], rows[0])
	if len(removals) != 1 || len(inspections) != 2 || len(r.sessionRemovals) != 0 {
		t.Fatalf("matching batch: %d removals, %d inspections, %d pending", len(removals), len(inspections), len(r.sessionRemovals))
	}
	for _, req := range inspections {
		if got := req.Command.Data.(protocol.CommandInspectRequestPayload).ID; got != removals[0].ID {
			t.Errorf("inspected %s, want the original request %s", got, removals[0].ID)
		}
	}
	remove(rows[0], rows[2])
	if len(removals) != 2 || removals[1].ID == removals[0].ID || len(inspections) != 2 {
		t.Fatalf("new batch: %d removals, %d inspections", len(removals), len(inspections))
	}
	want := []protocol.SessionRevision{{Session: snapshot.Sessions[0].ID, Revision: rows[0].Revision}, {Session: snapshot.Sessions[2].ID, Revision: rows[2].Revision}}
	if got := removals[1].Command.Data.(protocol.CommandRemoveSessionsPayload).Expected; !slices.Equal(got, want) {
		t.Fatalf("new batch sent %v, want %v", got, want)
	}
}

type slowRemovalTransport struct {
	*selectionTestTransport
	started    chan protocol.Request
	admit      chan struct{}
	admitted   chan struct{}
	completion chan link.Response
	calls      atomic.Int32
}

func (s *slowRemovalTransport) Dispatch(ctx context.Context, request protocol.Request) (link.Admission, error) {
	if request.Command.Kind != "remove_session" && request.Command.Kind != "remove_sessions" {
		return s.selectionTestTransport.Dispatch(ctx, request)
	}
	if s.calls.Add(1) != 1 {
		return link.Admission{}, fileActionFault(protocol.ErrorCodeConflict, "duplicate removal")
	}
	s.started <- request
	<-s.admit
	close(s.admitted)
	return link.Admission{Receipt: protocol.Receipt{ID: request.ID, Durable: true}, Completion: s.completion}, nil
}

func TestSessionRemovalDoesNotHoldNavigationOnBindingRetirement(t *testing.T) {
	for _, bulk := range []bool{false, true} {
		name := "single"
		if bulk {
			name = "bulk"
		}
		t.Run(name, func(t *testing.T) {
			w := selectionWindow(t, selectionSnapshot("Local"))
			snapshot := selectionSnapshot("Remote")
			snapshot.Node = protocol.NodeID{2}
			id := snapshot.Sessions[0].ID
			source := &slowRemovalTransport{
				selectionTestTransport: &selectionTestTransport{
					node:                      &conversationTestSubscription{updates: make(chan protocol.Update, 1), closed: make(chan struct{})},
					conversationTestTransport: conversationTestTransport{snapshot: snapshot},
				},
				started: make(chan protocol.Request, 1), admit: make(chan struct{}),
				admitted: make(chan struct{}), completion: make(chan link.Response, 1),
			}
			unblock := sync.OnceFunc(func() { close(source.admit) })
			complete := sync.OnceFunc(func() {
				source.completion <- link.Response{Output: protocol.Output{Kind: "sessions_removed", Data: []protocol.SessionID{id}}}
			})
			defer unblock()
			defer complete()
			local := panes.Target{Kind: panes.Session, ID: id.String()}
			prefs := w.r.App.Prefs()
			prefs.Tabs = []panes.Target{local}
			w.r.App.SetPrefs(prefs)
			source.node.updates <- protocol.Update{Kind: "snapshot", Data: snapshot}
			controller := client.New(source)
			if err := controller.Start(t.Context()); err != nil {
				t.Fatal(err)
			}
			t.Cleanup(func() { _ = controller.Close() })
			owner := w.r.newBinding(controller)
			host := snapshot.Node.String()
			w.r.bindings = map[string]*Runtime{host: owner}
			row := &viewmodel.Conversation{ID: id.String(), Node: host, Revision: 1}
			remove := func() { w.r.removeSession(row) }
			if bulk {
				w.r.App.Settings.ManagedNode = host
				w.r.backupSettings.shown, w.r.backupSettings.visible = owner, true
				remove = w.r.backupActions().PrepareDelete("")
				if remove == nil {
					t.Fatal("bulk removal unavailable")
				}
				w.r.App.Settings.ManagedNode = ""
				changed := snapshot
				changed.Sessions = slices.Clone(snapshot.Sessions)
				changed.Sessions[0].Revision++
				// Project the new revision before the confirmation is accepted.
				changed.Cursor++
				source.node.updates <- protocol.Update{Kind: "snapshot", Data: changed}
				deadline := time.Now().Add(time.Second)
				for owner.snapshot().Sessions[0].Revision != changed.Sessions[0].Revision {
					if time.Now().After(deadline) {
						t.Fatal("remote session update was not projected")
					}
					time.Sleep(time.Millisecond)
				}
			}
			remove()
			select {
			case req := <-source.started:
				if bulk {
					payload := req.Command.Data.(protocol.CommandRemoveSessionsPayload)
					if !slices.Equal(payload.Expected, backupRevisions(snapshot.Sessions, true)) {
						t.Fatalf("confirmation resampled the server revisions: %v", payload.Expected)
					}
				}
				if req.Target != snapshot.Node {
					t.Fatalf("deletion was sent to %s, want %s", req.Target, snapshot.Node)
				}
			case <-time.After(time.Second):
				t.Fatal("deletion did not reach the service")
			}
			row.Revision++
			remove()
			switched := make(chan struct{})
			go func() {
				w.r.App.Navigate(shell.SettingsPage)
				w.r.retireBinding(host, owner)
				w.window.Frame()
				close(switched)
			}()
			select {
			case <-switched:
			case <-time.After(time.Second):
				unblock()
				complete()
				<-switched
				t.Fatal("the UI could not switch pages until the deletion service returned")
			}
			if w.r.App.Page != shell.SettingsPage {
				t.Fatal("deletion stole focus")
			}
			unblock()
			select {
			case <-source.admitted:
			case <-time.After(time.Second):
				t.Fatal("deletion was not admitted")
			}
			w.r.App.Navigate(shell.ConversationPage)
			w.window.Frame()
			if w.r.App.Page != shell.ConversationPage || len(w.r.sessionRemovals) != 1 {
				t.Fatal("slow receipt blocked navigation or lost its pending removal")
			}
			complete()
			w.r.workers.Wait()
			w.window.Frame()
			if source.calls.Load() != 1 || len(w.r.sessionRemovals) != 0 {
				t.Fatalf("submitted %d removals, %d pending", source.calls.Load(), len(w.r.sessionRemovals))
			}
			if w.r.App.Data.Find(id.String()) == nil || !slices.Contains(w.r.App.Prefs().Tabs, local) {
				t.Fatal("remote deletion removed the local session with the same ID")
			}
		})
	}
}
