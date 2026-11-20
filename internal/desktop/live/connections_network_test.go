package live

import (
	"context"
	"slices"
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

func TestSavedConnectionRelayUsesShortAddressAndKeepsPin(t *testing.T) {
	const peer = "/p2p/QmS3zcG7LhYZYSJMhyRZvTddvbNUqtt8BJpaSs6mi1K5Va"
	const backup = "/dns4/backup.example/tcp/4001/p2p/QmSoLV4Bbm51jM9C4gDYZQ9Cy3U6aXMJDAbzgu2fzaDs64"
	for _, row := range []struct {
		name, primary, short string
	}{
		{"dns", "/dns4/primary.example/tcp/4001" + peer, "primary.example:4001"},
		{"ipv4", "/ip4/192.0.2.10/tcp/4101" + peer, "192.0.2.10:4101"},
		{"ipv6", "/ip6/2001:db8::7/udp/4201/quic-v1" + peer, "[2001:db8::7]:4201"},
	} {
		t.Run(row.name, func(t *testing.T) {
			saved := protocol.NetworkSelection{Kind: "custom", Data: protocol.NetworkSelectionCustomPayload{Relays: []string{row.primary, backup}}}
			r := &Runtime{ctx: t.Context(), connections: &connectionState{}, App: shell.NewLive(&viewmodel.Data{Workspace: &viewmodel.Workspace{}})}
			model := &settings.NetworkEditor{}
			picker := r.newConnectionPicker(nil, model, &saved)
			t.Cleanup(picker.close)
			if !picker.custom(model.Relay) || model.Address != row.short {
				t.Fatalf("saved relay draft = choice %d, address %q; want Custom with %q", model.Relay, model.Address, row.short)
			}
			selection, err := picker.draftSelection(model.Relay, model.Address)
			want := []string{row.primary, backup}
			payload, ok := detail[protocol.NetworkSelectionCustomPayload](selection.Data)
			if err != nil || selection.Kind != "custom" || !ok || !slices.Equal(payload.Relays, want) {
				t.Fatalf("selection from %q = %+v, %v; want both pinned relays %q", model.Address, selection, err, want)
			}
		})
	}
}

func TestConnectionCatalogKeepsOnlyFixedChoicesAndPreservesRoutes(t *testing.T) {
	const peer = "/p2p/QmS3zcG7LhYZYSJMhyRZvTddvbNUqtt8BJpaSs6mi1K5Va"
	const primary = "primary.example:4001"
	saved := protocol.NetworkSelection{Kind: "custom", Data: protocol.NetworkSelectionCustomPayload{Relays: []string{
		"/dns4/primary.example/tcp/4001" + peer,
		"/dns4/backup.example/tcp/4001/p2p/QmSoLV4Bbm51jM9C4gDYZQ9Cy3U6aXMJDAbzgu2fzaDs64",
	}}}
	preset := protocol.NetworkPreset{ID: "region", Label: protocol.PluginDesktopNavigation{Label: "Region"}, Selection: protocol.NetworkSelection{
		Kind: "custom", Data: protocol.NetworkSelectionCustomPayload{Relays: []string{"/dns4/listed.example/tcp/4001" + peer}},
	}}
	r := &Runtime{ctx: t.Context(), connections: &connectionState{}, App: shell.NewLive(&viewmodel.Data{Workspace: &viewmodel.Workspace{}})}
	model := &settings.NetworkEditor{}
	picker := r.newConnectionPicker(nil, model, &saved)
	t.Cleanup(func() {
		picker.close()
		r.workers.Wait()
	})
	assertChoices := func(want ...string) {
		t.Helper()
		var ids []string
		for _, choice := range picker.choices {
			ids = append(ids, choice.id)
		}
		if !slices.Equal(ids, want) {
			t.Fatalf("choices = %v, want %v", ids, want)
		}
	}
	assertChoice := func(want string) {
		t.Helper()
		index := model.Relay
		if index < 0 || index >= len(picker.choices) || picker.choices[index].id != want {
			t.Fatalf("selected choice = %d in %+v, want %q", index, picker.choices, want)
		}
	}
	assertCustomDraft := func(want string) {
		t.Helper()
		assertChoice("custom")
		if model.Address != want {
			t.Fatalf("custom draft address = %q, want %q", model.Address, want)
		}
	}
	assertSelection := func(want protocol.NetworkSelection) {
		t.Helper()
		got, err := picker.draftSelection(model.Relay, model.Address)
		if err != nil || !sameConnectionNetwork(&want, &got) {
			t.Fatalf("selection at %d with %q = %+v, %v; want %+v", model.Relay, model.Address, got, err, want)
		}
	}
	assertChoices("direct", "custom")
	assertCustomDraft(primary)
	assertSelection(saved)

	shadow := preset
	shadow.ID = "custom"
	r.acceptConnectionCatalog(picker, []protocol.NetworkPreset{shadow, {ID: "old-direct", Selection: protocol.NetworkSelection{Kind: "direct"}}, preset})
	assertChoices("direct", "preset/region", "custom")
	assertCustomDraft(primary)
	assertSelection(saved)

	known := preset
	known.ID, known.Selection = "saved", saved
	const edited = "edited.example:4001"
	model.Address = edited
	r.acceptConnectionCatalog(picker, []protocol.NetworkPreset{known, preset})
	assertChoices("direct", "preset/saved", "preset/region", "custom")
	assertCustomDraft(edited)
	assertSelection(protocol.NetworkSelection{Kind: "custom", Data: protocol.NetworkSelectionCustomPayload{Relays: []string{edited}}})

	model.Address = primary
	millis := uint64(41)
	picker.lookup = &connectionRelayLookup{key: encoded(saved), cancel: func() {}, ready: true, result: saved}
	picker.choices[model.Relay].millis = &millis
	tester := ui.NewTester(func(c *ui.Context) { r.connectionPickerFrame(c, picker) }, 800, 600)
	frame := func() {
		t.Helper()
		tester.Frame()
		if len(model.Relays) != len(picker.choices) {
			t.Fatalf("editor has %d relay options, want %d", len(model.Relays), len(picker.choices))
		}
	}
	frame()
	if shown := model.Relays[model.Relay].Millis; shown == nil || *shown != millis {
		t.Fatal("saved custom route should show its measured latency before matching")
	}

	r.acceptConnectionCatalog(picker, []protocol.NetworkPreset{known, preset})
	assertChoice("preset/saved")
	assertSelection(saved)
	matched := model.Relay
	model.Relay = len(picker.choices) - 1
	connectionTiming(picker, saved, false, &millis)
	frame()
	assertCustomDraft("")
	if shown := model.Relays[matched].Millis; shown == nil || *shown != millis {
		t.Fatal("matched preset should still show its measured latency")
	}
	if choice := model.Relays[model.Relay]; !choice.Custom || choice.Pending || choice.Millis != nil {
		t.Fatalf("Custom retained latency from the matched route: %+v", choice)
	}
	if _, err := picker.draftSelection(model.Relay, model.Address); faultCode(err) != protocol.ErrorCodeInvalidRequest {
		t.Fatalf("empty Custom draft was not rejected: %v", err)
	}

	model.Address = primary
	r.acceptConnectionCatalog(picker, []protocol.NetworkPreset{known, preset})
	assertCustomDraft(primary)
	assertSelection(protocol.NetworkSelection{Kind: "custom", Data: protocol.NetworkSelectionCustomPayload{Relays: []string{primary}}})

	model.Relay, model.Address = 2, ""
	assertSelection(preset.Selection)
	r.acceptConnectionCatalog(picker, nil)
	assertChoices("direct", "custom")
	assertCustomDraft("listed.example:4001")
	assertSelection(preset.Selection)
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
	t.Cleanup(editor.picker.close)
	load := &connectionCatalogLoad{done: make(chan struct{}), presets: []protocol.NetworkPreset{{ID: "fixture", Selection: pinned}}}
	close(load.done)
	editor.picker.catalog = load
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
	editor.picker.catalog.presets = []protocol.NetworkPreset{{ID: "fixture", Selection: protocol.NetworkSelection{Kind: "custom", Data: protocol.NetworkSelectionCustomPayload{Relays: []string{base + "QmSoLV4Bbm51jM9C4gDYZQ9Cy3U6aXMJDAbzgu2fzaDs64"}}}}}
	r.saveConnectionNetwork(model)
	r.workers.Wait()
	tester.Frame()
	receipts := source.sent("inspect_request")
	if len(receipts) != 2 || encoded(receipts[0]) != encoded(receipts[1]) || len(source.sent("save_network_settings")) != 1 || len(source.sent("probe_network")) != 1 || r.App.Settings.NetworkEditor != nil {
		t.Fatal("retry did not check the original save unchanged")
	}
}
