package live

import (
	"context"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/assembly/desktop"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// heldLoginTransport admits every begin_provider_login and completes all but
// the first at once; the first completes when the test releases it.
type heldLoginTransport struct {
	*conversationTestTransport
	first chan link.Response
}

func (t *heldLoginTransport) Dispatch(ctx context.Context, request protocol.Request) (link.Admission, error) {
	if request.Command.Kind != "begin_provider_login" {
		return t.conversationTestTransport.Dispatch(ctx, request)
	}
	input, _ := detail[protocol.CommandBeginProviderLoginPayload](request.Command.Data)
	completion := make(chan link.Response, 1)
	t.mu.Lock()
	t.requests = append(t.requests, request)
	if t.first == nil {
		t.first = completion
	} else {
		completion <- link.Response{Output: protocol.Output{Kind: "provider_login", Data: protocol.ConversationLoginAttempt{ID: request.ID, Provider: input.Provider}}}
	}
	t.mu.Unlock()
	return link.Admission{Receipt: protocol.Receipt{ID: request.ID}, Completion: completion}, nil
}

// testProviderLoginRestart closes a sign-in whose start the Node has not
// answered and starts it again. The second start is the same command: it is
// still sent and answered, and the attempt the Node then begins for the first
// start is cancelled, since nothing follows it.
func testProviderLoginRestart(t *testing.T) {
	provider := protocol.DefaultConversationProvider()
	provider.ID, provider.Name, provider.Api, provider.Enabled, provider.Revision, provider.Authentication = protocol.ProviderID{1}, "Account", protocol.ConversationModelApiPlugin, true, 1, protocol.AuthenticationPlugin
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node = protocol.NodeID{1}
	snapshot.Providers = []protocol.ConversationProvider{provider}
	transport := &heldLoginTransport{conversationTestTransport: &conversationTestTransport{snapshot: snapshot}}
	sent := func(kind string) (requests []protocol.Request) {
		transport.mu.Lock()
		defer transport.mu.Unlock()
		for _, request := range transport.requests {
			if request.Command.Kind == kind {
				requests = append(requests, request)
			}
		}
		return
	}
	controller := client.New(transport)
	if err := controller.Start(t.Context()); err != nil {
		t.Fatal(err)
	}
	defer controller.Close()
	assembly := desktop.Harness()
	catalog := i18n.New(assembly.Locales...)
	r, err := NewWithAssembly(t.Context(), controller, preferences.Open(t.TempDir()+"/preferences.json"), assembly)
	if err != nil {
		t.Fatal(err)
	}
	defer r.Close()
	m := r.App.Settings
	r.App.Page = shell.SettingsPage
	m.Open(settings.Providers)
	tester := ui.NewTesterWith(func(c *ui.Context) {
		i18n.Bind(c, catalog)
		r.mu.Lock()
		queue := r.queue
		r.queue = nil
		r.mu.Unlock()
		for _, post := range queue {
			post(c)
		}
		if len(queue) != 0 {
			r.delivered++
		}
		r.syncSettingsPages()
		settings.Page(c, m, r.App.Data, settings.Look{Theme: &r.App.Theme, Mode: &r.App.Mode, Effects: &r.App.Effects, Surfaces: &r.App.Surfaces})
		ui.ToastViewportBase(c, func(ui.Element, []ui.Toast) {})
	}, 1100, 900, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	tester.SetLocale("en")
	await := func(what string, done func() bool) {
		t.Helper()
		deadline := time.Now().Add(10 * time.Second)
		for tester.Frame(); !done(); tester.Frame() {
			if time.Now().After(deadline) {
				t.Fatalf("timed out waiting for %s", what)
			}
			time.Sleep(2 * time.Millisecond)
		}
	}
	id := provider.ID.String()
	await("the provider", func() bool { _, ok := r.loginProvider(id); return ok })

	m.ProviderActions.SignIn(r.controller.Target().String(), id)
	await("the first start", func() bool { return len(sent("begin_provider_login")) == 1 })
	view := r.App.ProviderUI.ProviderAuthorization()
	if !view.Open || view.Flow != settings.FlowStarting {
		t.Fatalf("the first sign-in is not starting: %+v", view)
	}
	view.Cancel()
	if view.Open || r.authorizations.provider != nil {
		t.Fatal("a sign-in the Node has not begun did not close")
	}

	m.ProviderActions.SignIn(r.controller.Target().String(), id)
	await("the second start", func() bool { return len(sent("begin_provider_login")) == 2 })
	starts := sent("begin_provider_login")
	await("the second attempt", func() bool {
		return r.authorizations.provider != nil && r.authorizations.provider.id == starts[1].ID
	})
	if len(sent("cancel_provider_login")) != 0 {
		t.Fatal("an attempt was cancelled before the superseded start answered")
	}

	transport.first <- link.Response{Output: protocol.Output{Kind: "provider_login", Data: protocol.ConversationLoginAttempt{ID: starts[0].ID, Provider: provider.ID}}}
	await("the superseded attempt's cancel", func() bool { return len(sent("cancel_provider_login")) == 1 })
	cancelled, _ := detail[protocol.CommandCancelProviderLoginPayload](sent("cancel_provider_login")[0].Command.Data)
	if cancelled.Attempt != starts[0].ID {
		t.Fatalf("cancelled attempt %s, want the superseded %s", cancelled.Attempt, starts[0].ID)
	}
	if current := r.authorizations.provider; current == nil || current.id != starts[1].ID || !r.App.ProviderUI.ProviderAuthorization().Open {
		t.Fatal("the superseded start's answer disturbed the current sign-in")
	}

	// Both Nodes expose the same provider ID. A settings refresh can also
	// remove its row while its browser callback is still pending.
	remoteSnapshot := snapshot
	remoteSnapshot.Node = protocol.NodeID{2}
	peer := &heldLoginTransport{
		conversationTestTransport: &conversationTestTransport{snapshot: remoteSnapshot},
		first:                     make(chan link.Response, 1),
	}
	remoteController := client.New(peer)
	if err := remoteController.Start(t.Context()); err != nil {
		t.Fatal(err)
	}
	defer remoteController.Close()
	remote := r.newBinding(remoteController)
	defer remote.Close()
	r.bindings = map[string]*Runtime{remoteSnapshot.Node.String(): remote}
	m.ProviderActions.SignIn(remoteSnapshot.Node.String(), id)
	await("the remote attempt", func() bool {
		return remote.authorizations != nil && remote.authorizations.provider != nil && remote.authorizations.provider.id != (protocol.RequestID{})
	})
	remoteAttempt := remote.authorizations.provider
	localAttempt := r.authorizations.provider
	projectAuthorization(localAttempt.view, protocol.ConversationLoginState{Kind: "cancelled"})
	if shown := r.App.ProviderUI.ProviderAuthorization(); shown != remoteAttempt.view || !shown.Open {
		t.Fatal("the local attempt closed the remote dialog")
	}
	view.Cancel()
	view.Submit("stale callback")
	if r.authorizations.provider != localAttempt || len(sent("cancel_provider_login")) != 1 {
		t.Fatal("a superseded dialog changed a later local attempt")
	}
	m.Channels = nil
	remoteAttempt.view.Flow, remoteAttempt.view.Callback, remoteAttempt.view.Pasted = settings.FlowBrowser, true, "remote callback"
	tester.Frame()
	tester.Advance(time.Second)
	tester.Frame()
	continueButton, ok := tester.AccessByRole(ui.RoleButton, catalog.In("en", "provider_login_continue"))
	if !ok {
		t.Fatal("the callback action was unavailable after the provider list changed")
	}
	tester.ClickAt(continueButton.Bounds.X+continueButton.Bounds.W/2, continueButton.Bounds.Y+continueButton.Bounds.H/2)
	tester.Frame()
	remote.workers.Wait()
	peer.mu.Lock()
	defer peer.mu.Unlock()
	for _, request := range peer.requests {
		if request.Command.Kind == "complete_provider_login" {
			callback, _ := detail[protocol.CommandCompleteProviderLoginPayload](request.Command.Data)
			if request.Target != remoteSnapshot.Node || callback.Attempt != remoteAttempt.id || callback.Callback.Expose() != "remote callback" || len(sent("complete_provider_login")) != 0 {
				t.Fatal("the callback changed its original Node or attempt")
			}
			return
		}
	}
	t.Fatal("the remote attempt did not receive the pasted callback")
}
