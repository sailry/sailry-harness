package live

import (
	"context"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// A network change the Node has no record of is decided by the settings the
// Node holds, so the editor never waits on an answer that cannot come.
func TestSettleConnectionNetwork(t *testing.T) {
	selected := protocol.NetworkSelection{Kind: "custom", Data: protocol.NetworkSelectionCustomPayload{Relays: []string{"https://relay.example"}}}
	previous := protocol.NetworkSelection{Kind: "direct"}
	for _, row := range []struct {
		name            string
		stored, running *protocol.NetworkSelection
		code            protocol.ErrorCode
	}{
		{"in_effect", &selected, &selected, ""},
		{"not_applied", &previous, &previous, protocol.ErrorCodeInternal},
		{"never_selected", nil, nil, protocol.ErrorCodeInternal},
		{"running_only", &previous, &selected, protocol.ErrorCodeRevisionConflict},
		{"stored_only", &selected, &previous, protocol.ErrorCodeRevisionConflict},
	} {
		t.Run(row.name, func(t *testing.T) {
			err := settleConnectionNetwork(protocol.NetworkSettings{Selection: row.stored, Effective: row.running}, selected)
			if row.code == "" {
				if err != nil {
					t.Fatalf("change in effect was reported as %v", err)
				}
				return
			}
			if faultCode(err) != row.code {
				t.Fatalf("fault = %v, want %s", err, row.code)
			}
			if faultCode(err) == protocol.ErrorCodeOutcomeUnknown {
				t.Fatal("a decided change must release the editor")
			}
		})
	}
}

func TestSaveConnectionNetworkPinsCurrentCustomDraft(t *testing.T) {
	const hostname = "relay.example"
	const port = ":4001"
	const base = "/dns4/" + hostname + "/tcp/4001/p2p/"
	pinned := protocol.NetworkSelection{Kind: "custom", Data: protocol.NetworkSelectionCustomPayload{Relays: []string{base + "QmS3zcG7LhYZYSJMhyRZvTddvbNUqtt8BJpaSs6mi1K5Va"}}}
	direct := protocol.NetworkSelection{Kind: "direct"}
	before := protocol.NetworkSettings{Revision: 1, Selection: &direct, Effective: &direct}
	after := protocol.Output{Kind: "network_settings", Data: protocol.NetworkSettings{Revision: 2, Selection: &pinned, Effective: &pinned}}
	var inspections int
	source := &terminalTabsTestNode{respond: func(request protocol.Request) link.Response {
		switch request.Command.Kind {
		case "probe_network":
			payload, _ := detail[protocol.CommandProbeNetworkPayload](request.Command.Data)
			return link.Response{Output: protocol.Output{Kind: "network_latency", Data: protocol.NetworkLatency{Selection: payload.Selection}}}
		case "save_network_settings":
			return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown}}
		case "inspect_request":
			inspections++
			payload, _ := detail[protocol.CommandInspectRequestPayload](request.Command.Data)
			outcome := protocol.RequestOutcome{Kind: "admitted"}
			if inspections == 2 {
				outcome = protocol.RequestOutcome{Kind: "completed", Data: protocol.Result[protocol.Output, protocol.Fault]{Ok: &after}}
			}
			return link.Response{Output: protocol.Output{Kind: "request_outcome", Data: protocol.OutputRequestOutcomePayload{ID: payload.ID, Outcome: outcome}}}
		case "read_network_settings":
			if inspections == 2 {
				return link.Response{Output: after}
			}
			return link.Response{Output: protocol.Output{Kind: "network_settings", Data: before}}
		default:
			return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeInternal}}
		}
	}}
	local := protocol.NodeID{1}
	controller := client.New(link.NewLocal(local, local, source))
	defer controller.Close()
	r := &Runtime{ctx: t.Context(), controller: controller, connections: &connectionState{}, App: shell.NewLive(&viewmodel.Data{Workspace: &viewmodel.Workspace{}})}
	r.App.Page = shell.SettingsPage
	r.App.Settings.Open(settings.Connections)
	ctx, cancel := context.WithCancel(r.ctx)
	defer cancel()
	model := &settings.NetworkEditor{}
	editor := &connectionNetworkEditor{model: model, ctx: ctx, cancel: cancel, controller: controller, saved: &before}
	r.connections.editor, r.App.Settings.NetworkEditor = editor, model
	editor.picker = r.newConnectionPicker(controller, model, &direct)
	editor.picker.presets = []protocol.NetworkPreset{{ID: "fixture", Selection: pinned}}
	tester := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		r.mu.Lock()
		queue := r.queue
		r.queue = nil
		r.mu.Unlock()
		for _, apply := range queue {
			apply(c)
		}
	}, 800, 600)

	model.Relay, model.Address = 1, "https://"+hostname+port
	r.saveConnectionNetwork(model)
	r.workers.Wait()
	if !model.Pending || len(source.sent("save_network_settings")) != 0 {
		t.Fatal("the save must wait for the resolver result on the UI thread")
	}
	model.Address = hostname + port
	tester.Frame()
	if model.Pending || model.Locked || len(source.sent("save_network_settings")) != 0 || len(source.sent("probe_network")) != 0 {
		t.Fatal("a result for the old draft was used")
	}

	r.saveConnectionNetwork(model)
	r.workers.Wait()
	tester.Frame()
	r.workers.Wait()
	tester.Frame()
	saves, probes := source.sent("save_network_settings"), source.sent("probe_network")
	if len(saves) != 1 || len(probes) != 1 || !model.Locked || !model.Retry || model.Address != hostname+port {
		t.Fatalf("custom save: %d saves, %d probes, editor %+v", len(saves), len(probes), model)
	}
	payload, _ := detail[protocol.CommandSaveNetworkSettingsPayload](saves[0].Data)
	if payload.ExpectedRevision != before.Revision || !sameConnectionNetwork(&payload.Selection, &pinned) {
		t.Fatalf("saved selection is not pinned: %+v", payload)
	}
	for _, command := range probes {
		probe, _ := detail[protocol.CommandProbeNetworkPayload](command.Data)
		if !sameConnectionNetwork(&probe.Selection, &payload.Selection) {
			t.Fatalf("probed selection differs from the save: %+v", probe)
		}
	}

	// A refreshed catalog cannot change the ID or payload of a pending save.
	editor.picker.cancelLookup()
	editor.picker.presets = []protocol.NetworkPreset{{ID: "fixture", Selection: protocol.NetworkSelection{Kind: "custom", Data: protocol.NetworkSelectionCustomPayload{Relays: []string{base + "QmSoLV4Bbm51jM9C4gDYZQ9Cy3U6aXMJDAbzgu2fzaDs64"}}}}}
	r.saveConnectionNetwork(model)
	r.workers.Wait()
	tester.Frame()
	receipts := source.sent("inspect_request")
	if len(receipts) != 2 || encoded(receipts[0]) != encoded(receipts[1]) || len(source.sent("save_network_settings")) != 1 || len(source.sent("probe_network")) != 1 || r.App.Settings.NetworkEditor != nil {
		t.Fatal("retry did not check the original save unchanged")
	}
}
