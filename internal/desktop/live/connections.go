package live

import (
	"context"
	"encoding/hex"
	"os"
	"runtime"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type connectionState struct {
	endpoint        *link.Link
	claim           link.PairingClaim
	pairCancel      context.CancelFunc
	pairGeneration  uint64
	picker          *connectionPicker
	editor          *connectionNetworkEditor
	visible         bool
	sharing         bool
	shareGeneration uint64
	shareCancel     context.CancelFunc
	shareSelection  string
	shareChange     *connectionNetworkChange
	localName       string
}

func (r *Runtime) initConnectionActions() {
	if r.parent != nil {
		return
	}
	if r.connections == nil {
		name, _ := os.Hostname()
		r.connections = &connectionState{localName: name}
	}
	state := r.connections
	if r.network != nil {
		state.endpoint = r.network.endpoint
	}
	model := r.App.Settings.Model
	model.ConnectionActions = &settings.ConnectionActions{
		Add:         r.addConnection,
		Revoke:      r.revokeConnection,
		EditNetwork: r.editConnectionNetwork,
		SaveNetwork: r.saveConnectionNetwork,
		RetryShare:  func() { r.startConnectionShare(0) },
		BrowseKey: func(selected func(string)) {
			if r.pickKeyFile == nil {
				r.enqueue(func(c *ui.Context) { kit.Toast(c, kit.Failed, i18n.T(c, "unavailable")) })
				return
			}
			r.pickKeyFile(func(path string) {
				if path != "" {
					r.enqueue(func(*ui.Context) { selected(path) })
				}
			})
		},
	}
	r.App.Hooks.EditHost = func(id string) {
		for _, device := range r.projectDevices() {
			if device.ID == id && device.Execution {
				r.App.Page = shell.SettingsPage
				r.App.Settings.Open(settings.Connections)
				r.editConnectionNetwork(device)
				return
			}
		}
	}
}

func (r *Runtime) connectionsFrame(c *ui.Context) {
	state := r.connections
	if state == nil {
		return
	}
	model := r.App.Settings.Model
	model.ConnectionStage = r.sshProgress()
	if r.App.Page != shell.SettingsPage || r.App.Settings.Section != settings.Connections {
		model.CloseConnectionDialogs()
	}
	visible := model.Adding() || model.Sharing()
	if visible != state.visible {
		state.visible = visible
		if visible {
			var selected *protocol.NetworkSelection
			if state.endpoint != nil {
				selected = state.endpoint.NetworkSelection()
			}
			state.picker = r.newConnectionPicker(r.controller, nil, selected)
			r.startConnectionCatalog(state.picker)
		} else if state.picker != nil {
			state.picker.close()
			state.picker = nil
		}
	}
	if !model.Adding() && state.pairCancel != nil {
		state.pairCancel()
		state.pairCancel = nil
		state.pairGeneration++
		model.ConnectionPending = false
	}
	if picker := state.picker; picker != nil {
		r.connectionPickerFrame(c, picker)
	}
	if model.Sharing() {
		draft := model.ConnectionDraft()
		selected, err := r.connectionSelection(draft)
		key := encoded(selected)
		if err != nil {
			key = draft.Address
		}
		if !state.sharing || state.shareSelection != key {
			delay := time.Duration(0)
			if state.sharing && state.picker != nil && state.picker.custom(draft.Relay) {
				delay = 650 * time.Millisecond
			}
			state.sharing, state.shareSelection = true, key
			r.startConnectionShare(delay)
		}
	} else if state.sharing {
		state.sharing = false
		r.stopConnectionShare()
		model.Share = settings.ShareState{}
		state.shareChange = nil
	}
	if editor := state.editor; editor != nil {
		if model.NetworkEditor != editor.model {
			editor.cancel()
			if editor.picker != nil {
				editor.picker.close()
			}
			state.editor = nil
		} else if editor.picker != nil {
			r.connectionPickerFrame(c, editor.picker)
		}
	}
}

func (r *Runtime) closeConnections() {
	if state := r.connections; state != nil {
		if state.pairCancel != nil {
			state.pairCancel()
		}
		if state.picker != nil {
			state.picker.close()
		}
		if state.editor != nil {
			state.editor.cancel()
			if state.editor.picker != nil {
				state.editor.picker.close()
			}
		}
		r.stopConnectionShare()
	}
}

func (r *Runtime) connectionSelection(draft settings.ConnectionDraft) (protocol.NetworkSelection, error) {
	if state := r.connections; state != nil && state.picker != nil {
		return state.picker.selection(draft.Relay, draft.Address)
	}
	if draft.Relay == 0 {
		return protocol.NetworkSelection{Kind: "iroh"}, nil
	}
	address := strings.TrimSpace(draft.Address)
	if address == "" {
		return protocol.NetworkSelection{}, connectionFault(protocol.ErrorCodeInvalidRequest, "relay network is required")
	}
	return protocol.NetworkSelection{Kind: "custom", Data: protocol.NetworkSelectionCustomPayload{Relays: []string{address}}}, nil
}

func (r *Runtime) addConnection(draft settings.ConnectionDraft, completed func(bool)) {
	if draft.Method != settings.ByPairing {
		r.addSSHHost(draft, completed)
		return
	}
	state := r.connections
	if state == nil || state.endpoint == nil {
		r.enqueue(func(c *ui.Context) {
			kit.Toast(c, kit.Failed, i18n.T(c, "unavailable"))
			completed(false)
		})
		return
	}
	if state.pairCancel != nil {
		completed(false)
		return
	}
	selected, err := r.connectionSelection(draft)
	if err != nil {
		r.enqueue(func(c *ui.Context) {
			kit.Toast(c, kit.Failed, i18n.T(c, "pairing_relay_invalid"))
			completed(false)
		})
		return
	}
	ctx, cancel := context.WithCancel(r.ctx)
	state.pairCancel = cancel
	state.pairGeneration++
	generation := state.pairGeneration
	r.App.Settings.ConnectionPending = true
	origin := r.Preferences.Data.Pairing
	endpoint := state.endpoint
	r.async(func() {
		defer cancel()
		_, err := endpoint.PairPINOnNetwork(ctx, origin, draft.Code, selected, &state.claim)
		r.enqueue(func(c *ui.Context) {
			if state.pairGeneration != generation {
				return
			}
			state.pairCancel = nil
			r.App.Settings.ConnectionPending = false
			if err != nil {
				outcome, key := kit.Failed, "pairing_failed"
				if faultCode(err) == protocol.ErrorCodeOutcomeUnknown {
					outcome, key = kit.Uncertain, "chat_action_unknown"
				} else if faultCode(err) == protocol.ErrorCodeConflict {
					outcome = kit.Conflict
				}
				kit.Toast(c, outcome, i18n.T(c, key))
			} else {
				kit.Toast(c, kit.Done, i18n.T(c, "pairing_success"))
			}
			completed(err == nil)
		})
	})
}

func (r *Runtime) revokeConnection(device viewmodel.Device) {
	if device.Local || r.connections == nil || r.connections.endpoint == nil {
		return
	}
	decoded, err := hex.DecodeString(device.ID)
	if err != nil || len(decoded) != len(protocol.NodeID{}) {
		return
	}
	var id protocol.NodeID
	copy(id[:], decoded)
	endpoint := r.connections.endpoint
	r.async(func() {
		err := endpoint.SetTrust(r.ctx, id, false)
		r.enqueue(func(c *ui.Context) {
			if err != nil {
				r.report(c, err, "pairing_failed")
			} else if r.Preferences.Data.Devices != nil {
				delete(*r.Preferences.Data.Devices, device.ID)
			}
		})
	})
}

func (r *Runtime) projectDevices() []viewmodel.Device {
	localID := r.controller.Target()
	platform := runtime.GOOS
	if platform == "darwin" {
		platform = "macos"
	}
	name := ""
	if r.connections != nil {
		name = r.connections.localName
	}
	// A Host's system version is what it answered inspect_host with.
	system := func(id string) string {
		if state := r.localRuntime().hosts[id]; state != nil && state.info != nil {
			return strings.TrimSpace(stringValue(state.info.OsVersion))
		}
		return ""
	}
	devices := []viewmodel.Device{{ID: localID.String(), Name: name, Platform: platform, Execution: true, Local: true, Online: r.controller.Connected(),
		OS: system(localID.String()), Version: link.ProductVersion}}
	known := map[string]preferences.Device{}
	if r.Preferences.Data.Devices != nil {
		known = *r.Preferences.Data.Devices
	}
	if r.network != nil {
		for _, peer := range r.network.snapshot() {
			id := peer.address.ID.String()
			remembered, exists := known[id]
			device := viewmodel.Device{ID: id, Execution: true, Online: peer.online}
			if exists || remembered.Name != nil || remembered.Platform != nil {
				device.Execution = remembered.Execution
				if remembered.Name != nil {
					device.Name = *remembered.Name
				}
				if remembered.Platform != nil {
					device.Platform = *remembered.Platform
				}
			}
			if peer.info.Platform != "" {
				device.Execution, device.Platform = peer.info.Execution, peer.info.Platform
				if peer.info.Name != nil {
					device.Name = *peer.info.Name
				}
				device.OS, device.Version = system(id), stringValue(peer.info.SoftwareVersion)
			}
			if device.Online && peer.connection.Route != nil {
				device.Route = string(*peer.connection.Route)
				if peer.connection.RTTMS != nil {
					device.RTT = int(*peer.connection.RTTMS)
				}
			}
			devices = append(devices, device)
		}
	}
	next := make(map[string]preferences.Device, len(known)+len(devices))
	for id, device := range known {
		next[id] = device
	}
	for _, device := range devices {
		remembered := preferences.Device{Execution: device.Execution}
		if device.Name != "" {
			value := device.Name
			remembered.Name = &value
		}
		if device.Platform != "" {
			value := device.Platform
			remembered.Platform = &value
		}
		next[device.ID] = remembered
	}
	r.Preferences.Data.Devices = &next
	r.App.Settings.Devices = devices
	return devices
}

func connectionFault(code protocol.ErrorCode, message string) error {
	return &protocol.Fault{Code: code, Message: message}
}

func (r *Runtime) reportConnection(c *ui.Context, err error, fallback string) {
	outcome, key := kit.Failed, fallback
	switch faultCode(err) {
	case protocol.ErrorCodeConflict, protocol.ErrorCodeRevisionConflict:
		outcome, key = kit.Conflict, "host_network_conflict"
	case protocol.ErrorCodeOutcomeUnknown:
		outcome, key = kit.Uncertain, "host_network_unknown"
	}
	kit.Toast(c, outcome, i18n.T(c, key))
}
