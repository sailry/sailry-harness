package live

import (
	"context"
	"encoding/hex"
	"encoding/json"
	"reflect"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

func TestPluginCardPackageLifecycle(t *testing.T) {
	const entry = "dev.sailry.platform/desktop/card.js"
	node, session, worktree, turn := protocol.NodeID{1}, protocol.SessionID{2}, protocol.WorktreeID{3}, protocol.TurnID{4}
	info := protocol.DefaultPluginInfo()
	info.Summary.Name, info.Summary.Digest, info.Summary.Enabled = "card-fixture", strings.Repeat("a", 64), true
	info.Summary.Compatibility.Kind = "compatible"
	info.Summary.Revision, info.Summary.SettingsRevision = 1, 1
	declaration := protocol.DefaultPluginCardDeclaration()
	declaration.Key, declaration.Entry, declaration.McpServer = "fixture", entry, new("fixture")
	declaration.PropsSchema = json.RawMessage(`{"type":"object","properties":{"title":{"type":"string","maxLength":80}},"required":["title"]}`)
	declaration.Capabilities.Browser = []protocol.PluginCardTarget{{Source: protocol.PluginCardSource{Kind: "mcp_authorization"}}}
	info.Extension = new(protocol.DefaultPluginExtension())
	info.Extension.Cards = []protocol.PluginCardDeclaration{declaration}
	captured := protocol.DefaultPluginCardSummary()
	captured.Node, captured.Worktree = node, worktree
	captured.Receipt = protocol.PluginCardReceipt{Session: session, Turn: turn, Call: protocol.NewRequestID()}
	captured.Package, captured.Key = packageReference(info.Summary), declaration.Key
	captured.Props = json.RawMessage(`{"title":"Captured card title"}`)
	captured.Fallback.Label = "Captured fallback"
	bundle := protocol.PluginDesktopBundle{Package: captured.Package, Entry: entry, Files: map[string]string{entry: `
import {View, div} from "gpui-kit";
import {Action, state, nextChange} from "sailry/card";
export default class extends View {
  init() { const card = state(); this.title = card.props.title; nextChange(card.cursor); }
  render() {
    return div().v_flex().child(this.title)
      .child(Action.new("cancel", {label:"Cancel authorization",action:{kind:"cancel"}}))
      .child(Action.new("check", {label:"Check authorization",action:{kind:"connect"}}));
  }
}`}}
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node, snapshot.Plugins = node, []protocol.PluginSummary{info.Summary}
	sessionRow := protocol.DefaultSession()
	sessionRow.ID, sessionRow.Worktree, sessionRow.Revision = session, worktree, 1
	snapshot.Sessions = []protocol.Session{sessionRow}
	history := protocol.DefaultConversationSnapshot()
	history.Node, history.Sequence, history.Page.Session, history.Page.Revision = node, 1, session, 1
	run := protocol.DefaultConversationRun()
	run.Session, run.Worktree, run.Turn, run.Sequence, run.Revision, run.Kind, run.Status = session, worktree, turn, 1, 1, "task", "completed"
	history.Page.Runs = []protocol.ConversationRun{run}
	attempt := protocol.PluginConnectorAttempt{ID: protocol.NewRequestID(), Connection: protocol.NewRequestID(), Package: captured.Package, Revision: 1}
	cancelled := protocol.Output{Kind: "connector_login", Data: attempt}
	transport := &pluginCardLifecycleTransport{
		conversationTestTransport: &conversationTestTransport{snapshot: snapshot, conversation: history},
		inventory:                 &conversationTestSubscription{updates: make(chan protocol.Update, 1), closed: make(chan struct{})},
		login:                     &conversationTestSubscription{updates: make(chan protocol.Update, 1), closed: make(chan struct{})},
		cancelled:                 make(chan link.Response, 1),
		observed:                  make(chan struct{}, 1),
	}
	transport.handle = func(request protocol.Request) (link.Response, bool) {
		switch request.Command.Kind {
		case "read_plugin_version":
			read, ok := detail[protocol.CommandReadPluginVersionPayload](request.Command.Data)
			if !ok || read.Package != captured.Package {
				return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "wrong captured package"}}, true
			}
			return link.Response{Output: protocol.Output{Kind: "plugin", Data: info}}, true
		case "read_plugin_view":
			read, ok := detail[protocol.CommandReadPluginViewPayload](request.Command.Data)
			receipt, valid := detail[protocol.PluginCardReceipt](read.Surface.Data)
			if !ok || !valid || read.Package != captured.Package || read.Surface.Kind != "card" || receipt != captured.Receipt {
				return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "wrong captured view"}}, true
			}
			return link.Response{Output: protocol.Output{Kind: "plugin_view", Data: bundle}}, true
		default:
			return controllerLoginTestOutcome(request, protocol.RequestOutcome{Kind: "completed", Data: protocol.Result[protocol.Output, protocol.Fault]{Ok: &cancelled}})
		}
	}
	controller := client.New(transport)
	if err := controller.Start(t.Context()); err != nil {
		t.Fatal(err)
	}
	defer controller.Close()
	wait := func(done <-chan struct{}, message string) {
		t.Helper()
		select {
		case <-done:
		case <-time.After(2 * time.Second):
			t.Fatal(message)
		}
	}
	ready := make(chan struct{}, 1)
	release, err := controller.WatchConversation(t.Context(), session, func() {
		if _, ok := controller.LatestLocalTurnStatus(session, turn); ok {
			select {
			case ready <- struct{}{}:
			default:
			}
		}
	})
	if err != nil {
		t.Fatal(err)
	}
	defer release()
	wait(ready, "Client did not project the card's conversation")
	root := &Runtime{ctx: t.Context(), controller: controller, App: shell.NewLive(&viewmodel.Data{Workspace: &viewmodel.Workspace{}}),
		Preferences: preferences.Open(t.TempDir() + "/preferences.json")}
	defer root.closePluginCards()
	const callID = "fixture-call"
	root.bindPluginCards(session.String(), callID, toolPluginCards(&protocol.ToolContent{
		Version: 1, Blocks: []protocol.ToolContentBlock{{Kind: "card", Data: captured}},
	}))
	key := pluginCardKey{node: node, receipt: captured.Receipt}
	card := root.pluginCards.cards[key]
	if card == nil {
		t.Fatal("valid captured card was not registered")
	}
	window := ui.NewTesterWith(func(view *ui.Context) {
		theme.Apply(view, "", theme.Light)
		root.mu.Lock()
		queue := root.queue
		root.queue = nil
		root.mu.Unlock()
		for _, apply := range queue {
			apply(view)
		}
		root.pluginCardsFrame(view)
		ui.Column(view).Fill().Children(func() { root.renderPluginCard(view, card) })
		root.pluginCardsAfterFrame()
		kit.ToastHost(view)
	}, 640, 420, ui.TesterOptions{Preferences: ui.Preferences{ReduceMotion: true, ReduceTransparency: true}})
	settle := func() {
		window.Frame()
		window.Frame()
		root.workers.Wait()
		window.Frame()
	}
	publish := func(summary protocol.PluginSummary) {
		t.Helper()
		snapshot.Cursor++
		snapshot.Plugins = []protocol.PluginSummary{summary}
		cursor := snapshot.Cursor
		applied := make(chan struct{}, 1)
		defer controller.OnChange(func() {
			if root.snapshot().Cursor == cursor {
				select {
				case applied <- struct{}{}:
				default:
				}
			}
		})()
		transport.inventory.updates <- protocol.Update{Kind: "snapshot", Data: snapshot}
		wait(applied, "Client did not publish the plugin inventory update")
	}
	click := func(label string) {
		t.Helper()
		if err := window.Click(label); err != nil {
			t.Fatal(err)
		}
		window.Frame()
	}
	requests := func() []protocol.Request {
		transport.mu.Lock()
		defer transport.mu.Unlock()
		return slices.Clone(transport.requests)
	}
	settle()
	if card.mounted == nil || !window.HasText("Captured card title") {
		t.Fatalf("captured card did not mount: %v", window.Texts())
	}
	root.observePluginCardLogin(card, attempt)
	wait(transport.observed, "card did not create an authorization observer")
	click("Cancel authorization")
	original := card.login.request
	if original == nil || !card.login.pending {
		t.Fatal("the authorization cancellation did not start")
	}
	transport.cancelled <- link.Response{Output: cancelled}
	root.workers.Wait()
	for cycle, change := range []func(*protocol.PluginSummary){
		func(summary *protocol.PluginSummary) { summary.Enabled = false },
		func(summary *protocol.PluginSummary) { summary.Digest = strings.Repeat("b", 64) },
	} {
		prior, priorHost, priorCtx := card.mounted, card.host, card.ctx
		priorLogin := priorHost.State().Login
		generation := card.generation
		requestCount := len(requests())
		retired := info.Summary
		retired.Revision = uint64(2*cycle + 2)
		change(&retired)
		publish(retired)
		settle()
		if card.runtime != nil || card.host != nil || card.mounted != nil || card.loading || card.cancel != nil ||
			priorCtx.Err() == nil || prior.Pending() != 0 || !prior.Suspended() || priorHost.State().Login != priorLogin {
			t.Fatal("retired package retained its card execution or applied the old authorization completion")
		}
		wait(transport.login.closed, "retired card retained its authorization observer")
		root.observers.Wait()
		if !window.HasText("Captured fallback") || window.HasText("Captured card title") || len(requests()) != requestCount ||
			root.pluginCards.cards[key] != card || len(root.pluginCards.calls) != 1 || !reflect.DeepEqual(card.summary, captured) ||
			card.login.request != original || card.login.attempt == nil || *card.login.attempt != attempt || card.login.state != "unconfirmed" {
			t.Fatal("retired card lost its capture or original authorization, or requested another mount")
		}
		restored := info.Summary
		restored.Revision, restored.SettingsRevision = retired.Revision+1, retired.Revision+1
		publish(restored)
		settle()
		if card.mounted == nil || card.mounted == prior || card.generation == generation || !window.HasText("Captured card title") ||
			card.scope().Package != packageReference(restored) || len(requests()) != requestCount+2 {
			t.Fatal("restoring the captured version did not mount it once with the current settings reference")
		}
	}
	click("Check authorization")
	settle()
	if card.login.request != nil || card.login.attempt != nil || card.login.state != "cancelled" {
		t.Fatal("the restored card did not consume the original cancellation outcome")
	}
	body, err := json.Marshal(original)
	if err != nil {
		t.Fatal(err)
	}
	digest := blake3.Sum256(body)
	writes, inspections := 0, 0
	for _, request := range requests() {
		switch request.Command.Kind {
		case "cancel_connector_login":
			writes++
		case "inspect_request":
			inspections++
			query, valid := detail[protocol.CommandInspectRequestPayload](request.Command.Data)
			if !valid || request.Target != node || query.ID != original.ID || query.Digest != hex.EncodeToString(digest[:]) {
				t.Fatal("authorization recovery did not inspect the original request on its Node")
			}
		}
	}
	if writes != 1 || inspections != 1 {
		t.Fatalf("authorization dispatches: %d writes, %d inspections", writes, inspections)
	}
}

type pluginCardLifecycleTransport struct {
	*conversationTestTransport
	inventory, login *conversationTestSubscription
	cancelled        chan link.Response
	observed         chan struct{}
}

func (transport *pluginCardLifecycleTransport) Subscribe(ctx context.Context, topic protocol.Topic) (link.Subscription, error) {
	switch topic.Kind {
	case "node":
		transport.inventory.updates <- protocol.Update{Kind: "snapshot", Data: transport.snapshot}
		return transport.inventory, nil
	case "connector_login":
		transport.observed <- struct{}{}
		return transport.login, nil
	default:
		return transport.conversationTestTransport.Subscribe(ctx, topic)
	}
}

func (transport *pluginCardLifecycleTransport) Dispatch(ctx context.Context, request protocol.Request) (link.Admission, error) {
	if request.Command.Kind != "cancel_connector_login" {
		return transport.conversationTestTransport.Dispatch(ctx, request)
	}
	transport.mu.Lock()
	transport.requests = append(transport.requests, request)
	transport.mu.Unlock()
	return link.Admission{Receipt: protocol.Receipt{ID: request.ID, Durable: true}, Completion: transport.cancelled}, nil
}
