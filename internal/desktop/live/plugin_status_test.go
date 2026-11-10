package live

import (
	"encoding/json"
	"strings"
	"testing"
	"testing/synctest"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// testPluginStatusClick covers a button badge's captured action: one request
// while busy, a settled failure, and an uncertain result retried by identity.
func testPluginStatusClick(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		fixture := newPluginLocationTestFixture(t)
		root := fixture.root
		declaration := protocol.DefaultPluginUiContribution()
		declaration.ID, declaration.Slot, declaration.Kind = "badge", protocol.PluginUiSlotStatus, protocol.PluginUiKindButton
		declaration.Label.Label, declaration.Handler = "Badge", new("click")
		state := protocol.DefaultPluginUiState()
		state.ID, state.Visible, state.Enabled = "badge", true, true
		entry := protocol.PluginUiStatusEntry{
			Package:     protocol.PluginReference{Name: "status-fixture", Digest: strings.Repeat("a", 64)},
			Declaration: declaration, State: state, Invoke: new("invoke"),
		}
		scope := protocol.PluginUiStatusScope{Worktree: &fixture.main.ID}
		body, _ := json.Marshal(scope)
		feed := &pluginStatusFeed{scope: scope, open: map[string]bool{}, pending: map[string]*pluginStatusPending{}, cancel: func() {}}
		feed.view.Entries = []protocol.PluginUiStatusEntry{entry}
		root.pluginStatus = map[string]*pluginStatusFeed{string(body): feed}
		id := pluginStatusIdentity(entry)
		context := protocol.PluginContext{Worktree: scope.Worktree}
		if root.invokePluginStatus(context, id) || len(fixture.source.commands) != 0 {
			t.Fatal("a disconnected badge started its action")
		}
		feed.view.Connected = true
		var requests []protocol.Request
		held := make(chan link.Response, 1)
		fixture.source.dispatch = func(request protocol.Request) (link.Admission, error) {
			requests = append(requests, request)
			switch len(requests) {
			case 1:
				return link.Admission{Receipt: protocol.Receipt{ID: request.ID}, Completion: held}, nil
			case 2:
				return link.Admission{}, &link.FaultError{Fault: protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "completion was lost"}}
			}
			completion := make(chan link.Response, 1)
			completion <- link.Response{Output: protocol.Output{Kind: "plugin_result", Data: json.RawMessage("null")}}
			return link.Admission{Receipt: protocol.Receipt{ID: request.ID}, Completion: completion}, nil
		}
		frame := pluginLocationTestFrame(root)
		click := func() {
			if !root.invokePluginStatus(context, id) {
				t.Fatal("badge click was not accepted")
			}
			synctest.Wait()
		}
		click()
		if root.invokePluginStatus(context, id) {
			t.Fatal("a busy badge accepted a second click")
		}
		synctest.Wait()
		if len(requests) != 1 || requests[0].Command.Kind != "call_plugin" || requests[0].Plugin == nil || requests[0].Plugin.Package != entry.Package {
			t.Fatalf("a busy badge repeated or rescoped its action: %+v", requests)
		}
		held <- link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "fixture conflict"}}
		synctest.Wait()
		frame()
		if feed.pending[id] != nil {
			t.Fatal("a settled failure retained its request")
		}
		click()
		frame()
		if pending := feed.pending[id]; pending == nil || pending.active || len(requests) != 2 {
			t.Fatal("an uncertain action lost its captured request")
		}
		click()
		frame()
		if len(requests) != 3 || requests[2].ID != requests[1].ID || requests[1].ID == requests[0].ID || feed.pending[id] != nil {
			t.Fatalf("retry did not reuse the uncertain request identity: %+v", requests)
		}
	})
}
