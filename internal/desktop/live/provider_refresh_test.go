package live

import (
	"context"
	"slices"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestProviderStartupWaitsForCredentialMetadata(t *testing.T) {
	for _, test := range []struct {
		name                          string
		revoked, expired, missing     bool
		readFailed, signedIn, pending bool
	}{
		{name: "connected", signedIn: true},
		{name: "revoked", revoked: true},
		{name: "expired", expired: true},
		{name: "missing", missing: true},
		{name: "read unavailable", readFailed: true, pending: true},
	} {
		t.Run(test.name, func(t *testing.T) {
			snapshot := protocol.DefaultSnapshot()
			snapshot.Node = protocol.NodeID{1}
			account := providerRouteFixture()
			account.Authentication = protocol.AuthenticationChatGptOAuth
			account.Credential = &protocol.CredentialRef{Node: snapshot.Node, ID: protocol.CredentialID{3}}
			snapshot.Providers = []protocol.ConversationProvider{account}
			expires := uint64(time.Now().Add(time.Hour).UnixMilli())
			if test.expired {
				expires = 1
			}
			values := []protocol.Credential{}
			if !test.missing {
				values = append(values, protocol.Credential{ID: account.Credential.ID, Provider: account.ID,
					Authentication: account.Authentication, Revoked: test.revoked, ExpiresAtMs: &expires})
			}
			transport := &conversationTestTransport{snapshot: snapshot}
			transport.handle = func(request protocol.Request) (link.Response, bool) {
				if request.Command.Kind != "list_credentials" {
					return link.Response{}, false
				}
				if test.readFailed {
					return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "credential metadata temporarily unavailable"}}, true
				}
				return link.Response{Output: protocol.Output{Kind: "credentials", Data: values}}, true
			}
			controller := client.New(transport)
			if err := controller.Start(t.Context()); err != nil {
				t.Fatal(err)
			}
			defer controller.Close()
			r, err := NewWithPreferences(t.Context(), controller, preferences.Open(t.TempDir()+"/preferences.json"))
			if err != nil {
				t.Fatal(err)
			}
			defer r.Close()
			var toasts []ui.Toast
			tester := ui.NewTester(func(c *ui.Context) {
				r.drain(c)
				r.finishFrame()
				ui.ToastViewportBase(c, func(_ ui.Element, values []ui.Toast) { toasts = slices.Clone(values) })
			}, 640, 400)
			tester.SetLocale("en")
			r.workers.Wait()
			tester.Frame()
			baseline := len(toasts)
			r.readProviderCredentials()
			pending := r.projectNodeProviders()[0]
			if !pending.AccountPending || pending.SignedIn || settings.ProviderStatus(&pending) != "" {
				t.Fatal("first projection reported sign-out before credential metadata arrived")
			}
			r.workers.Wait()
			tester.Frame()
			channel := r.projectNodeProviders()[0]
			wantStatus := ""
			if !test.signedIn && !test.pending {
				wantStatus = "provider_status_signed_out"
			}
			if channel.SignedIn != test.signedIn || channel.AccountPending != test.pending || settings.ProviderStatus(&channel) != wantStatus {
				t.Fatalf("unexpected account state after metadata response: %+v", channel)
			}
			if len(toasts) != baseline {
				t.Fatalf("background account metadata read produced an action toast: %+v", toasts)
			}
		})
	}
}

func TestProviderLoginRefreshesOpenDraft(t *testing.T) {
	for _, modelFailure := range []bool{false, true} {
		name := "models discovered"
		if modelFailure {
			name = "model discovery failed"
		}
		t.Run(name, func(t *testing.T) {
			provider := providerRouteFixture()
			provider.Enabled = true
			account := provider
			account.ID, account.Authentication, account.Credential = protocol.ProviderID{9}, protocol.AuthenticationChatGptOAuth, nil
			snapshot := protocol.DefaultSnapshot()
			snapshot.Node, snapshot.Providers = protocol.NodeID{1}, []protocol.ConversationProvider{provider, account}
			transport := &conversationTestTransport{snapshot: snapshot}
			controller := client.New(transport)
			if err := controller.Start(t.Context()); err != nil {
				t.Fatal(err)
			}
			defer controller.Close()
			r, err := NewWithPreferences(t.Context(), controller, preferences.Open(t.TempDir()+"/preferences.json"))
			if err != nil {
				t.Fatal(err)
			}
			defer r.Close()
			id := r.App.FocusedConversation()
			cv, comp := r.App.Data.Find(id), r.App.Composer(id)
			var offered *composer.Context
			var toasts []ui.Toast
			tester := ui.NewTester(func(c *ui.Context) {
				r.drain(c)
				offered, _ = r.conversation(c, cv, comp)
				r.finishFrame()
				ui.ToastViewportBase(c, func(_ ui.Element, values []ui.Toast) { toasts = slices.Clone(values) })
			}, 640, 400)
			tester.SetLocale("en")
			settle := func() {
				for range 6 {
					r.workers.Wait()
					tester.Frame()
				}
			}
			settle()
			toastCount := len(toasts)
			comp.Draft = "Keep this draft"
			selected := comp.Model
			account.Credential = &protocol.CredentialRef{Node: snapshot.Node, ID: protocol.CredentialID{9}}
			account.Revision++
			transport.snapshot.Cursor++
			transport.snapshot.Providers = []protocol.ConversationProvider{provider, account}
			ctx, cancel := context.WithCancel(t.Context())
			defer cancel()
			view := r.App.Settings.ProviderAuthorization()
			view.Open = true
			attempt := &liveAuthorization{provider: account.ID.String(), ctx: ctx, cancel: cancel, view: view}
			r.authorizations.provider = attempt
			r.enqueue(func(c *ui.Context) {
				r.applyProviderLogin(c, attempt, protocol.ConversationLoginUpdate{
					State: protocol.ConversationLoginState{Kind: "exchanging"}, Credential: account.Credential,
				})
			})
			settle()
			if view.Open || view.Busy || ctx.Err() != nil || len(toasts) != toastCount {
				t.Fatal("saved credential did not dismiss the dialog while keeping discovery active")
			}
			update := protocol.ConversationLoginUpdate{State: protocol.ConversationLoginState{Kind: "connected"}, Credential: account.Credential}
			if modelFailure {
				update.ModelError = &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "fixture model discovery failed"}
			}
			r.enqueue(func(c *ui.Context) { r.applyProviderLogin(c, attempt, update) })
			settle()
			if !slices.ContainsFunc(offered.Models, func(model composer.Model) bool { return model.ID == modelKey(account.ID, account.Models[0].ID) }) {
				t.Fatal("login did not refresh credential-backed models without a subscription event")
			}
			if view.Open || ctx.Err() == nil || comp.Model != selected || comp.Draft != "Keep this draft" {
				t.Fatal("login did not close cleanly or replaced the draft")
			}
			wantType, wantKey := "success", "authorization_connected"
			if modelFailure {
				wantType, wantKey = "warning", "provider_login_models_failed"
			}
			if len(toasts) != toastCount+1 || toasts[toastCount].Type != wantType || toasts[toastCount].Title != i18n.In("en", wantKey) {
				t.Fatalf("login outcome toasts: %+v", toasts)
			}
		})
	}
}

func TestSavedProviderRefreshesOpenDraft(t *testing.T) {
	for _, removed := range []bool{false, true} {
		name := "saved channel appears without a subscription event"
		if removed {
			name = "a later removal stays authoritative"
		}
		t.Run(name, func(t *testing.T) {
			provider := providerRouteFixture()
			provider.Enabled = true
			snapshot := protocol.DefaultSnapshot()
			snapshot.Node, snapshot.Providers = protocol.NodeID{1}, []protocol.ConversationProvider{provider}
			snapshot.ProviderDirectory = true
			transport := &conversationTestTransport{snapshot: snapshot}
			var saved protocol.ConversationProvider
			transport.handle = func(request protocol.Request) (link.Response, bool) {
				if request.Command.Kind == "read_provider_directory" {
					directory := protocol.ProviderDirectory{Node: snapshot.Node, Cursor: transport.snapshot.Cursor, Providers: transport.snapshot.Providers}
					return link.Response{Output: protocol.Output{Kind: "provider_directory", Data: directory}}, true
				}
				if request.Command.Kind != "save_provider" {
					return link.Response{}, false
				}
				input, _ := detail[protocol.CommandSaveProviderPayload](request.Command.Data)
				saved = input.Provider
				saved.Revision = 1
				// Hold the subscription at its original cursor through the save.
				transport.snapshot.Cursor++
				if !removed {
					transport.snapshot.Providers = append(slices.Clone(transport.snapshot.Providers), saved)
				}
				return link.Response{Output: protocol.Output{Kind: "provider", Data: saved}}, true
			}
			controller := client.New(transport)
			if err := controller.Start(t.Context()); err != nil {
				t.Fatal(err)
			}
			defer controller.Close()
			r, err := NewWithPreferences(t.Context(), controller, preferences.Open(t.TempDir()+"/preferences.json"))
			if err != nil {
				t.Fatal(err)
			}
			defer r.Close()
			id := r.App.FocusedConversation()
			cv := r.App.Data.Find(id)
			if cv == nil {
				t.Fatal("new conversation draft is unavailable")
			}
			comp := r.App.Composer(id)
			var offered *composer.Context
			tester := ui.NewTester(func(c *ui.Context) {
				r.drain(c)
				offered, _ = r.conversation(c, cv, comp)
				r.finishFrame()
				ui.ToastViewportBase(c, func(ui.Element, []ui.Toast) {})
			}, 640, 400)
			settle := func() {
				for range 6 {
					r.workers.Wait()
					tester.Frame()
				}
			}
			settle()
			comp.Draft = "Keep the existing conversation draft"
			selected := comp.Model
			newID := protocol.ProviderID{9}.String()
			result := settings.ProviderSaveResult{}
			r.App.Settings.ProviderActions.Save(settings.ProviderDraft{Channel: viewmodel.Channel{
				ID: newID, Name: "New channel", API: "responses", Endpoint: "https://models.example.test/v1", Auth: viewmodel.AuthKey, Enabled: true,
				Models: []viewmodel.ChannelModel{{ID: "new-model", Context: 128000, Output: 16384}},
			}}, func(outcome settings.ProviderSaveResult) {
				result = outcome
				found := slices.ContainsFunc(r.App.Data.Models, func(model composer.Model) bool { return model.ID == modelKey(saved.ID, "new-model") })
				if found == removed {
					t.Error("the save completed before the model choices reflected the committed directory")
				}
			})
			settle()
			if len(result.Succeeded) != 1 {
				t.Fatal("confirmed channel save was not reported")
			}
			found := slices.ContainsFunc(offered.Models, func(model composer.Model) bool { return model.ID == modelKey(saved.ID, "new-model") })
			if found == removed {
				t.Fatalf("open draft model choices do not match the fresh Node snapshot: %+v", offered.Models)
			}
			if comp.Model != selected || comp.Draft != "Keep the existing conversation draft" {
				t.Fatal("refreshing channel choices replaced the draft or its model selection")
			}
			transport.mu.Lock()
			defer transport.mu.Unlock()
			if count := len(slices.DeleteFunc(slices.Clone(transport.requests), func(request protocol.Request) bool { return request.Command.Kind != "save_provider" })); count != 1 {
				t.Fatalf("refresh repeated the channel save: %d requests", count)
			}
		})
	}
}
