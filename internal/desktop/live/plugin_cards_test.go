package live

import (
	"strings"
	"testing"
	"testing/synctest"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func testPluginCardContracts(t *testing.T) {
	t.Run("failed card can be revisited after its toast expires", testPluginCardRevisit)
	target := protocol.PluginCardTarget{Origins: []string{"https://example.test"}}
	redirect := "http://127.0.0.1:43191/callback"
	for _, current := range []struct {
		address string
		valid   bool
	}{
		{"https://example.test/authorize?state=private", true},
		{"https://example.test:443/authorize", true},
		{"https://example.test.evil/authorize", false},
		{"https://user@example.test/authorize", false},
		{"http://example.test/authorize", false},
		{"javascript:alert(1)", false},
		{redirect + "?code=private&state=private", true},
		{"http://127.0.0.1:43192/callback?code=private", false},
		{"http://127.0.0.1:43191/other?code=private", false},
		{redirect + "#private", false},
	} {
		if pluginCardBrowserAllowed(current.address, target, &redirect) != current.valid {
			t.Fatalf("card browser origin or callback confinement changed: %s", current.address)
		}
	}
	request := protocol.NewRequest(protocol.NodeID{1}, protocol.Command{Kind: "cancel_connector_login", Data: protocol.CommandCancelConnectorLoginPayload{Attempt: protocol.NewRequestID()}})
	login := pluginCardAuthorization{
		request: &request, state: "pending", url: "https://example.test/authorize?state=private", expires: 101,
		redirect: &redirect, returned: make(chan string),
	}
	if !login.canOpen(time.UnixMilli(100)) || login.canOpen(time.UnixMilli(101)) {
		t.Fatal("card authorization URL ignored its exact expiry")
	}
	login.suspend()
	if login.request == nil || login.request.ID != request.ID || login.url != "" || login.redirect != nil ||
		login.returned != nil || login.state != "unconfirmed" || login.keepsAlive(time.UnixMilli(100)) {
		t.Fatal("card suspension lost the original uncertain request or retained private callback state")
	}
	for _, code := range []protocol.ErrorCode{protocol.ErrorCodeOutcomeUnknown, protocol.ErrorCodeUnavailable, protocol.ErrorCodeBusy, protocol.ErrorCodeInternal, protocol.ErrorCodeCancelled} {
		if !pluginCardUncertain(code) {
			t.Fatalf("card lost an uncertain durable request: %s", code)
		}
		failure := &protocol.Fault{Code: code, Message: "isolated completed outcome"}
		state := client.RequestFailed
		if code == protocol.ErrorCodeOutcomeUnknown {
			// Recorded by the Node as never to be known: final as well.
			state = client.RequestUnknown
		}
		_, err, confirmed := pluginCardInspected(client.Resolution{State: state, Fault: failure}, nil)
		if !confirmed || err == nil || faultCode(err) != code {
			t.Fatal("completed card fault was not definitive")
		}
	}
	// Only a request the Node still runs, or a Node that cannot be asked,
	// keeps the login unconfirmed; every other answer ends the request, so a
	// card never asks about the same request forever.
	for name, inspected := range map[string]struct {
		resolution client.Resolution
		failure    error
		confirmed  bool
		code       protocol.ErrorCode
	}{
		"running":       {client.Resolution{State: client.RequestPending}, nil, false, protocol.ErrorCodeOutcomeUnknown},
		"unreachable":   {client.Resolution{}, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "Node is unreachable"}, false, protocol.ErrorCodeUnavailable},
		"interrupted":   {client.Resolution{State: client.RequestUnknown, Fault: &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "the Node restarted"}}, nil, true, protocol.ErrorCodeOutcomeUnknown},
		"never arrived": {client.Resolution{State: client.RequestAbsent}, nil, true, protocol.ErrorCodeUnavailable},
		"applied":       {client.Resolution{State: client.RequestApplied, Output: protocol.Output{Kind: "connector_login"}}, nil, true, ""},
	} {
		output, err, confirmed := pluginCardInspected(inspected.resolution, inspected.failure)
		if confirmed != inspected.confirmed || faultCode(err) != inspected.code || (err == nil) != (output.Kind == "connector_login") {
			t.Fatalf("%s card inspection = %#v, %v, confirmed %t", name, output, err, confirmed)
		}
	}
	reference := protocol.PluginReference{Name: "captured-card", Digest: strings.Repeat("a", 64), SettingsRevision: 2}
	card := &pluginCardInstance{summary: protocol.PluginCardSummary{Package: reference}, reference: reference}
	replacement := reference
	replacement.Digest, replacement.SettingsRevision = strings.Repeat("b", 64), 20
	card.updateReference(replacement)
	if card.scope().Package != reference {
		t.Fatal("card capture followed the current inventory replacement")
	}
	replacement = reference
	replacement.SettingsRevision++
	card.updateReference(replacement)
	if card.scope().Package != replacement || card.summary.Package != reference {
		t.Fatal("card settings reference mutated the canonical captured summary")
	}
}

func testPluginCardRevisit(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		node, session, worktree, turn := protocol.NodeID{1}, protocol.SessionID{2}, protocol.WorktreeID{3}, protocol.TurnID{4}
		config := protocol.DefaultSessionConfig()
		config.Provider, config.Model, config.Mode, config.Permission = protocol.ProviderID{5}, "fixture", "code", "ask"
		snapshot := protocol.DefaultSnapshot()
		snapshot.Node = node
		snapshot.Sessions = []protocol.Session{{ID: session, Worktree: worktree, Revision: 1, Config: config, Roles: protocol.DefaultRoleSnapshot()}}
		history := protocol.DefaultConversationSnapshot()
		history.Node, history.Sequence, history.Page.Session, history.Page.Revision = node, 1, session, 1
		run := protocol.DefaultConversationRun()
		run.Session, run.Worktree, run.Turn, run.Sequence, run.Revision, run.Kind, run.Status = session, worktree, turn, 1, 1, "task", "completed"
		history.Page.Runs = []protocol.ConversationRun{run}
		transport := &conversationTestTransport{snapshot: snapshot, conversation: history, handle: func(request protocol.Request) (link.Response, bool) {
			if request.Command.Kind != "read_plugin_version" {
				return link.Response{}, false
			}
			return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "Fixture card could not load"}}, true
		}}
		controller := client.New(transport)
		if err := controller.Start(t.Context()); err != nil {
			t.Fatal(err)
		}
		defer controller.Close()
		release, err := controller.WatchConversation(t.Context(), session, func() {})
		if err != nil {
			t.Fatal(err)
		}
		defer release()
		synctest.Wait()
		if state, ok := controller.Conversation(session); !ok || !state.Connected || state.Err != nil {
			t.Fatalf("fixture conversation is not ready: %+v", state)
		}
		root := &Runtime{ctx: t.Context(), controller: controller, Preferences: preferences.Open(t.TempDir() + "/preferences.json")}
		defer root.closePluginCards()
		summary := protocol.DefaultPluginCardSummary()
		summary.Node, summary.Worktree = node, worktree
		summary.Receipt = protocol.PluginCardReceipt{Session: session, Turn: turn, Call: protocol.RequestID{6}}
		summary.Package = protocol.PluginReference{Name: "fixture-card", Digest: strings.Repeat("a", 64)}
		summary.Key, summary.Fallback.Label = "fixture", "Card fallback"
		card := &pluginCardInstance{key: pluginCardKey{node: node, receipt: summary.Receipt}, owner: root, summary: summary, reference: summary.Package}
		root.initPluginCards().cards[card.key] = card
		presentations := 2
		window := ui.NewTester(func(view *ui.Context) {
			theme.Apply(view, "", theme.Light)
			kit.SetPreferences(view, kit.Preferences{ToastSeconds: 3})
			root.mu.Lock()
			queue := root.queue
			root.queue = nil
			root.mu.Unlock()
			for _, apply := range queue {
				apply(view)
			}
			ui.Column(view).Fill().Children(func() {
				for index := range presentations {
					root.renderPluginCard(view.Key(index), card)
				}
			})
			kit.ToastHost(view)
		}, 600, 400)
		window.SetLocale("en")
		window.SetPreferences(ui.Preferences{ReduceMotion: true, ReduceTransparency: true})
		settle := func() {
			for range 4 {
				window.Frame()
				synctest.Wait()
			}
		}
		requests := func() int {
			transport.mu.Lock()
			defer transport.mu.Unlock()
			return len(transport.requests)
		}
		settle()
		if requests() != 1 || card.failure == nil || !card.attempted || !window.HasText(i18n.In("en", "plugin_initialization_failed")) {
			t.Fatalf("first visit did not retain its failure: requests=%d failure=%v attempted=%t", requests(), card.failure, card.attempted)
		}
		window.Advance(4 * time.Second)
		settle()
		if requests() != 1 || window.HasText(i18n.In("en", "plugin_initialization_failed")) || !window.HasText("Card fallback") {
			t.Fatal("toast expiry retried the card or removed its fallback")
		}
		presentations = 1
		settle()
		if requests() != 1 || !card.attempted {
			t.Fatal("closing one pane retried a card still shown in another")
		}
		presentations = 0
		settle()
		if card.attempted || card.failure != nil {
			t.Fatal("leaving the last failed presentation kept its attempt locked")
		}
		presentations = 1
		settle()
		if requests() != 2 || card.failure == nil || !card.attempted || !window.HasText(i18n.In("en", "plugin_initialization_failed")) {
			t.Fatalf("reopening did not make exactly one new attempt: requests=%d failure=%v attempted=%t", requests(), card.failure, card.attempted)
		}
	})
}
