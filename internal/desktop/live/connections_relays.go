package live

import (
	"context"
	"strings"
	"sync"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type connectionChoice struct {
	id        string
	selection *protocol.NetworkSelection
	label     protocol.PluginDesktopNavigation
	pending   bool
	millis    *uint64
}

type connectionPicker struct {
	controller   *client.Client
	editor       *settings.NetworkEditor
	ctx          context.Context
	cancel       context.CancelFunc
	choices      []connectionChoice
	saved        *protocol.NetworkSelection
	customKey    string
	customCancel context.CancelFunc
}

func (picker *connectionPicker) close() {
	picker.cancel()
	if picker.customCancel != nil {
		picker.customCancel()
	}
}

func (r *Runtime) newConnectionPicker(controller *client.Client, editor *settings.NetworkEditor, selected *protocol.NetworkSelection) *connectionPicker {
	parent := r.ctx
	if editor != nil && r.connections.editor != nil {
		parent = r.connections.editor.ctx
	}
	ctx, cancel := context.WithCancel(parent)
	if selected == nil {
		value := protocol.NetworkSelection{Kind: "iroh"}
		selected = &value
	}
	picker := &connectionPicker{
		controller: controller, editor: editor, ctx: ctx, cancel: cancel, saved: selected,
		choices: []connectionChoice{{id: "retained", selection: selected}, {id: "custom"}},
	}
	address := ""
	if payload, ok := detail[protocol.NetworkSelectionCustomPayload](selected.Data); ok && len(payload.Relays) > 0 {
		address = payload.Relays[0]
	}
	picker.setDraft(r.App.Settings.Model, 0, address)
	return picker
}

func (picker *connectionPicker) draft(model *settings.Model) (int, string) {
	if picker.editor != nil {
		return picker.editor.Relay, picker.editor.Address
	}
	draft := model.ConnectionDraft()
	return draft.Relay, draft.Address
}

func (picker *connectionPicker) setDraft(model *settings.Model, index int, address string) {
	if picker.editor != nil {
		picker.editor.Relay, picker.editor.Address = index, address
	} else {
		model.SetConnectionNetwork(index, address)
	}
}

func (picker *connectionPicker) custom(index int) bool {
	return index >= 0 && index < len(picker.choices) && picker.choices[index].id == "custom"
}

func (picker *connectionPicker) selection(index int, address string) (protocol.NetworkSelection, error) {
	if index < 0 || index >= len(picker.choices) {
		return protocol.NetworkSelection{}, connectionFault(protocol.ErrorCodeInvalidRequest, "relay network is required")
	}
	if selected := picker.choices[index].selection; selected != nil {
		return *selected, nil
	}
	address = strings.TrimSpace(address)
	if address == "" {
		return protocol.NetworkSelection{}, connectionFault(protocol.ErrorCodeInvalidRequest, "relay network is required")
	}
	if picker.saved != nil {
		if payload, ok := detail[protocol.NetworkSelectionCustomPayload](picker.saved.Data); ok && len(payload.Relays) > 0 && payload.Relays[0] == address {
			return *picker.saved, nil
		}
	}
	return protocol.NetworkSelection{Kind: "custom", Data: protocol.NetworkSelectionCustomPayload{Relays: []string{address}}}, nil
}

func (r *Runtime) connectionPickerActive(picker *connectionPicker) bool {
	if picker.ctx.Err() != nil || r.connections == nil {
		return false
	}
	if picker.editor == nil {
		return r.connections.picker == picker
	}
	return r.connections.editor != nil && r.connections.editor.picker == picker && r.App.Settings.NetworkEditor == picker.editor
}

func (r *Runtime) connectionPickerFrame(c *ui.Context, picker *connectionPicker) {
	model := r.App.Settings.Model
	choices := make([]settings.RelayChoice, 0, len(picker.choices))
	for _, choice := range picker.choices {
		value := settings.RelayChoice{Label: choice.label.Label, Pending: choice.pending, Millis: choice.millis, Custom: choice.id == "custom"}
		if choice.label.Label == "" {
			value.LabelKey = "pairing_relay_selected"
			if choice.id == "custom" {
				value.LabelKey = "pairing_relay_custom"
			} else if choice.selection != nil && choice.selection.Kind == "iroh" {
				value.LabelKey = "pairing_relay_iroh"
			}
		} else if translated := choice.label.Locales[i18n.Match(c.Locale())]; translated != "" {
			value.Label = translated
		}
		choices = append(choices, value)
	}
	if picker.editor != nil {
		picker.editor.Relays = choices
	} else {
		model.ConnectionRelays = choices
	}
	index, address := picker.draft(model)
	key := ""
	if picker.custom(index) {
		key = strings.TrimSpace(address)
	}
	if key == picker.customKey {
		return
	}
	picker.customKey = key
	if picker.customCancel != nil {
		picker.customCancel()
		picker.customCancel = nil
	}
	custom := &picker.choices[len(picker.choices)-1]
	custom.pending, custom.millis = false, nil
	presented := &choices[len(choices)-1]
	presented.Pending, presented.Millis = false, nil
	if key == "" {
		return
	}
	selected, err := picker.selection(index, address)
	if err != nil {
		return
	}
	custom.pending = true
	presented.Pending = true
	ctx, cancel := context.WithCancel(picker.ctx)
	picker.customCancel = cancel
	r.async(func() {
		timer := time.NewTimer(650 * time.Millisecond)
		defer timer.Stop()
		select {
		case <-ctx.Done():
			return
		case <-timer.C:
		}
		millis := connectionProbe(ctx, picker.controller, selected)
		r.enqueue(func(*ui.Context) {
			if r.connectionPickerActive(picker) && ctx.Err() == nil && picker.customKey == key {
				custom := &picker.choices[len(picker.choices)-1]
				custom.pending, custom.millis = false, millis
			}
		})
	})
}

func (r *Runtime) startConnectionCatalog(picker *connectionPicker) {
	index, address := picker.draft(r.App.Settings.Model)
	selected, selectedErr := picker.selection(index, address)
	r.async(func() {
		var pending sync.WaitGroup
		limit := make(chan struct{}, 4)
		seen := map[string]bool{}
		probe := func(selected protocol.NetworkSelection) {
			key := encoded(selected)
			if seen[key] {
				return
			}
			seen[key] = true
			r.enqueue(func(*ui.Context) {
				if r.connectionPickerActive(picker) {
					connectionTiming(picker, selected, true, nil)
				}
			})
			pending.Add(1)
			go func() {
				defer pending.Done()
				select {
				case <-picker.ctx.Done():
					return
				case limit <- struct{}{}:
				}
				millis := connectionProbe(picker.ctx, picker.controller, selected)
				<-limit
				r.enqueue(func(*ui.Context) {
					if r.connectionPickerActive(picker) {
						connectionTiming(picker, selected, false, millis)
					}
				})
			}()
		}
		accept := func(listing protocol.NetworkListing) {
			r.enqueue(func(*ui.Context) {
				if r.connectionPickerActive(picker) {
					r.acceptConnectionCatalog(picker, listing.Presets)
				}
			})
			for _, preset := range listing.Presets {
				probe(preset.Selection)
			}
		}
		if listing, ok := connectionCatalog(picker.ctx, picker.controller, false); ok {
			accept(listing)
		}
		if selectedErr == nil {
			probe(selected)
		}
		if listing, ok := connectionCatalog(picker.ctx, picker.controller, true); ok {
			accept(listing)
		}
		pending.Wait()
	})
}

func connectionCatalog(ctx context.Context, controller *client.Client, refresh bool) (protocol.NetworkListing, bool) {
	output, err := controller.Do(ctx, protocol.Command{Kind: "list_network_presets", Data: protocol.CommandListNetworkPresetsPayload{Refresh: refresh}})
	if err != nil || output.Kind != "network_presets" {
		return protocol.NetworkListing{}, false
	}
	return detail[protocol.NetworkListing](output.Data)
}

func connectionProbe(ctx context.Context, controller *client.Client, selected protocol.NetworkSelection) *uint64 {
	output, err := controller.Do(ctx, protocol.Command{Kind: "probe_network", Data: protocol.CommandProbeNetworkPayload{Selection: selected}})
	if err != nil || output.Kind != "network_latency" {
		return nil
	}
	latency, ok := detail[protocol.NetworkLatency](output.Data)
	if !ok || !sameConnectionNetwork(&latency.Selection, &selected) {
		return nil
	}
	return latency.Millis
}

func connectionTiming(picker *connectionPicker, selected protocol.NetworkSelection, pending bool, millis *uint64) {
	for index := range picker.choices {
		if sameConnectionNetwork(picker.choices[index].selection, &selected) {
			picker.choices[index].pending, picker.choices[index].millis = pending, millis
		}
	}
}

func (r *Runtime) acceptConnectionCatalog(picker *connectionPicker, presets []protocol.NetworkPreset) {
	index, address := picker.draft(r.App.Settings.Model)
	previous := picker.choices
	var selected *connectionChoice
	if index >= 0 && index < len(previous) {
		value := previous[index]
		selected = &value
	}
	choices := make([]connectionChoice, 0, len(presets)+2)
	for _, preset := range presets {
		value := preset.Selection
		choice := connectionChoice{id: "preset/" + preset.ID, selection: &value, label: preset.Label}
		for _, old := range previous {
			if sameConnectionNetwork(old.selection, choice.selection) {
				choice.pending, choice.millis = old.pending, old.millis
				break
			}
		}
		choices = append(choices, choice)
	}
	chosen := -1
	if selected != nil && selected.selection != nil {
		for index, choice := range choices {
			if sameConnectionNetwork(choice.selection, selected.selection) && (selected.id == "retained" || choice.id == selected.id) {
				chosen = index
				break
			}
		}
		if chosen < 0 {
			retained := *selected
			retained.id, retained.label = "retained", protocol.PluginDesktopNavigation{}
			choices = append([]connectionChoice{retained}, choices...)
			chosen = 0
		}
	}
	custom := previous[len(previous)-1]
	choices = append(choices, custom)
	if selected != nil && selected.id == "custom" {
		chosen = len(choices) - 1
	}
	picker.choices = choices
	picker.setDraft(r.App.Settings.Model, max(0, chosen), address)
}

func sameConnectionNetwork(first, second *protocol.NetworkSelection) bool {
	return encoded(first) == encoded(second)
}
