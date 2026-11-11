package live

import (
	"context"
	"encoding/json"
	"errors"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type connectionNetworkEditor struct {
	model      *settings.NetworkEditor
	controller *client.Client
	ctx        context.Context
	cancel     context.CancelFunc
	saved      *protocol.NetworkSettings
	picker     *connectionPicker
	change     *connectionNetworkChange
}

type connectionNetworkChange struct {
	controller *client.Client
	previous   protocol.NetworkSettings
	selected   protocol.NetworkSelection
	request    protocol.Request
}

func (r *Runtime) connectionClient(device viewmodel.Device) (*client.Client, error) {
	if device.ID == r.controller.Target().String() {
		return r.controller, nil
	}
	if r.network != nil {
		for _, peer := range r.network.snapshot() {
			if peer.address.ID.String() != device.ID {
				continue
			}
			if peer.controller != nil {
				return peer.controller, nil
			}
			return client.New(r.network.endpoint.Remote(peer.address)), nil
		}
	}
	return nil, connectionFault(protocol.ErrorCodeNotFound, "host is not paired")
}

func (r *Runtime) editConnectionNetwork(device viewmodel.Device) {
	if !device.Execution || r.connections == nil {
		return
	}
	if previous := r.connections.editor; previous != nil {
		previous.cancel()
		if previous.picker != nil {
			previous.picker.close()
		}
	}
	ctx, cancel := context.WithCancel(r.ctx)
	editor := &connectionNetworkEditor{model: &settings.NetworkEditor{Device: device}, ctx: ctx, cancel: cancel}
	r.connections.editor = editor
	r.App.Settings.NetworkEditor = editor.model
	r.loadConnectionNetwork(editor)
}

func (r *Runtime) loadConnectionNetwork(editor *connectionNetworkEditor) {
	if editor.model.Loading || editor.model.Pending {
		return
	}
	editor.model.Loading, editor.model.Retry = true, false
	controller, err := r.connectionClient(editor.model.Device)
	if err != nil {
		editor.model.Loading, editor.model.Retry = false, true
		r.enqueue(func(c *ui.Context) {
			if r.App.Settings.NetworkEditor == editor.model {
				kit.Toast(c, kit.Failed, i18n.T(c, "host_network_load_failed"))
			}
		})
		return
	}
	editor.controller = controller
	r.async(func() {
		value, err := readConnectionNetwork(editor.ctx, controller)
		r.enqueue(func(c *ui.Context) {
			if editor.ctx.Err() != nil || r.App.Settings.NetworkEditor != editor.model {
				return
			}
			editor.model.Loading = false
			if err != nil {
				editor.model.Retry = true
				kit.Toast(c, kit.Failed, i18n.T(c, "host_network_load_failed"))
				return
			}
			editor.saved = &value
			editor.model.Revision = value.Revision
			editor.model.Unavailable = value.Effective == nil
			selected := value.Effective
			if selected == nil {
				selected = value.Selection
			}
			if editor.picker != nil {
				editor.picker.close()
			}
			editor.picker = r.newConnectionPicker(controller, editor.model, selected)
			r.startConnectionCatalog(editor.picker)
			if editor.model.Unavailable {
				kit.Toast(c, kit.Failed, i18n.T(c, "host_network_unavailable"))
			}
		})
	})
}

func (r *Runtime) saveConnectionNetwork(model *settings.NetworkEditor) {
	if r.connections == nil || r.connections.editor == nil {
		return
	}
	editor := r.connections.editor
	if editor.model != model || model.Loading || model.Pending || model.Unavailable {
		return
	}
	if editor.saved == nil || editor.picker == nil {
		r.loadConnectionNetwork(editor)
		return
	}
	selected, err := editor.picker.selection(model.Relay, model.Address)
	if err != nil {
		r.enqueue(func(c *ui.Context) { kit.Toast(c, kit.Failed, i18n.T(c, "pairing_relay_invalid")) })
		return
	}
	inspect := editor.change != nil
	if editor.change == nil {
		command := protocol.Command{Kind: "save_network_settings", Data: protocol.CommandSaveNetworkSettingsPayload{ExpectedRevision: editor.saved.Revision, Selection: selected}}
		editor.change = &connectionNetworkChange{controller: editor.controller, previous: *editor.saved, selected: selected, request: editor.controller.Prepare(command)}
	}
	change := editor.change
	model.Pending, model.Locked = true, true
	r.async(func() {
		actual, err := r.applyConnectionNetwork(editor.ctx, change, inspect)
		var refreshed *protocol.NetworkSettings
		if err != nil && editor.ctx.Err() == nil {
			if value, readErr := readConnectionNetwork(editor.ctx, editor.controller); readErr == nil {
				refreshed = &value
			}
		}
		r.enqueue(func(c *ui.Context) {
			if editor.ctx.Err() != nil || r.App.Settings.NetworkEditor != model {
				return
			}
			model.Pending = false
			if err == nil {
				editor.saved = &actual
				r.App.Settings.NetworkEditor = nil
				kit.Toast(c, kit.Done, i18n.T(c, "host_network_saved"))
				return
			}
			if refreshed != nil {
				editor.saved = refreshed
				model.Revision = refreshed.Revision
			}
			model.Locked = faultCode(err) == protocol.ErrorCodeOutcomeUnknown
			model.Retry = model.Locked
			if !model.Locked {
				editor.change = nil
			}
			r.reportConnection(c, err, "host_network_failed")
		})
	})
}

func readConnectionNetwork(ctx context.Context, controller *client.Client) (protocol.NetworkSettings, error) {
	output, err := controller.Do(ctx, protocol.Command{Kind: "read_network_settings"})
	if err != nil {
		return protocol.NetworkSettings{}, err
	}
	value, ok := detail[protocol.NetworkSettings](output.Data)
	if output.Kind != "network_settings" || !ok {
		return protocol.NetworkSettings{}, connectionFault(protocol.ErrorCodeInvalidRequest, "network settings response expected")
	}
	return value, nil
}

func connectionNetworkAddress(settings protocol.NetworkSettings, target protocol.NodeID) (link.EndpointAddress, error) {
	var address link.EndpointAddress
	if json.Unmarshal([]byte(settings.Address), &address) != nil {
		return address, connectionFault(protocol.ErrorCodeInvalidRequest, "invalid network address")
	}
	if address.ID != target {
		return address, connectionFault(protocol.ErrorCodeWrongTarget, "network address belongs to another Node")
	}
	return address, nil
}

// errConnectionNetworkUnrecorded: the Node can never report what the request
// did, so the settings it holds now decide the change.
var errConnectionNetworkUnrecorded = errors.New("host network change has no recorded outcome")

// recoverConnectionNetwork asks the Node what became of a change whose reply
// was not observed. It never sends the change again.
func (r *Runtime) recoverConnectionNetwork(ctx context.Context, change *connectionNetworkChange) (protocol.Output, error) {
	resolution, err := change.controller.Resolve(ctx, change.request)
	if err != nil && change.controller.Target() != r.controller.Target() {
		previous, addressErr := connectionNetworkAddress(change.previous, change.controller.Target())
		if addressErr != nil {
			return protocol.Output{}, unknownConnectionNetwork()
		}
		candidate, addressErr := link.CandidateAddress(previous, change.selected)
		if addressErr != nil || r.connections == nil || r.connections.endpoint == nil {
			return protocol.Output{}, unknownConnectionNetwork()
		}
		if err := r.connections.endpoint.UpdatePeer(ctx, candidate); err != nil {
			return protocol.Output{}, unknownConnectionNetwork()
		}
		resolution, err = change.controller.Resolve(ctx, change.request)
	}
	if err != nil {
		return protocol.Output{}, unknownConnectionNetwork()
	}
	switch resolution.State {
	case client.RequestApplied:
		return resolution.Output, nil
	case client.RequestFailed:
		return protocol.Output{}, resolution.Fault
	case client.RequestAbsent:
		return protocol.Output{}, connectionFault(protocol.ErrorCodeInvalidRequest, "network change was not admitted")
	case client.RequestUnknown:
		return protocol.Output{}, errConnectionNetworkUnrecorded
	}
	return protocol.Output{}, unknownConnectionNetwork()
}

// settleConnectionNetwork decides a change the Node has no record of from the
// settings it holds now. Both the stored and the running selection match: the
// change is in effect. Neither matches: it did not take effect. One matches:
// the Node holds something neither side asked for, which is a conflict; the
// editor reloads and the next save brings both back in line.
func settleConnectionNetwork(actual protocol.NetworkSettings, selected protocol.NetworkSelection) error {
	stored, running := sameConnectionNetwork(actual.Selection, &selected), sameConnectionNetwork(actual.Effective, &selected)
	switch {
	case stored && running:
		return nil
	case !stored && !running:
		return connectionFault(protocol.ErrorCodeInternal, "host network change did not take effect")
	}
	return connectionFault(protocol.ErrorCodeRevisionConflict, "host network settings changed")
}

func (r *Runtime) applyConnectionNetwork(ctx context.Context, change *connectionNetworkChange, inspect bool) (protocol.NetworkSettings, error) {
	var output protocol.Output
	var err error
	if inspect {
		output, err = r.recoverConnectionNetwork(ctx, change)
	} else {
		output, err = change.controller.Execute(ctx, change.request)
		if code := faultCode(err); code == protocol.ErrorCodeOutcomeUnknown || code == protocol.ErrorCodeUnavailable {
			output, err = r.recoverConnectionNetwork(ctx, change)
		}
	}
	var applied protocol.NetworkSettings
	if errors.Is(err, errConnectionNetworkUnrecorded) {
		// A Node that restarted runs what it stored, so its settings are the
		// answer its ledger no longer has.
		if applied, err = readConnectionNetwork(ctx, change.controller); err != nil {
			return protocol.NetworkSettings{}, unknownConnectionNetwork()
		}
		if err = settleConnectionNetwork(applied, change.selected); err != nil {
			return protocol.NetworkSettings{}, err
		}
	} else {
		if err != nil {
			return protocol.NetworkSettings{}, err
		}
		var ok bool
		if applied, ok = detail[protocol.NetworkSettings](output.Data); !ok || output.Kind != "network_settings" {
			return protocol.NetworkSettings{}, unknownConnectionNetwork()
		}
	}
	if change.controller.Target() != r.controller.Target() {
		address, err := connectionNetworkAddress(applied, change.controller.Target())
		if err != nil || r.connections == nil || r.connections.endpoint == nil {
			return protocol.NetworkSettings{}, unknownConnectionNetwork()
		}
		if err := r.connections.endpoint.UpdatePeer(ctx, address); err != nil {
			return protocol.NetworkSettings{}, unknownConnectionNetwork()
		}
	}
	actual, err := readConnectionNetwork(ctx, change.controller)
	if err != nil {
		return protocol.NetworkSettings{}, unknownConnectionNetwork()
	}
	if !sameConnectionNetwork(actual.Selection, &change.selected) || !sameConnectionNetwork(actual.Effective, &change.selected) {
		if actual.Revision > applied.Revision {
			return protocol.NetworkSettings{}, connectionFault(protocol.ErrorCodeRevisionConflict, "host network settings changed")
		}
		return protocol.NetworkSettings{}, unknownConnectionNetwork()
	}
	return actual, nil
}

func unknownConnectionNetwork() error {
	return connectionFault(protocol.ErrorCodeOutcomeUnknown, "host network change could not be confirmed")
}
