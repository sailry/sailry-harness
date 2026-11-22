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
