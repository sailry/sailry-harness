package live

import (
	"context"
	"errors"
	"io"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (r *Runtime) stopConnectionShare() {
	if state := r.connections; state != nil {
		state.shareGeneration++
		if state.shareCancel != nil {
			state.shareCancel()
			state.shareCancel = nil
		}
	}
}

func (r *Runtime) connectionShareDraft() (protocol.NetworkSelection, string, error) {
	draft := r.App.Settings.ConnectionDraft()
	selected, err := r.connectionDraftSelection(draft)
	if err != nil {
		return selected, draft.Address, err
	}
	return selected, encoded(selected), nil
}

func (r *Runtime) startConnectionShare(delay time.Duration, retry bool) {
	state := r.connections
	if state == nil || !r.connectionsOpen() || !r.App.Settings.Sharing() {
		return
	}
	r.stopConnectionShare()
	input, draftKey, err := r.connectionShareDraft()
	state.sharing, state.shareSelection = true, draftKey
	generation := state.shareGeneration
	current := func() bool {
		if state.shareGeneration != generation || !r.connectionsOpen() || !r.App.Settings.Sharing() {
			return false
		}
		_, key, _ := r.connectionShareDraft()
		return draftKey == key
	}
	if err != nil || state.endpoint == nil {
		r.App.Settings.Share = settings.ShareState{Kind: "failed"}
		r.enqueue(func(c *ui.Context) {
			if current() {
				kit.Toast(c, kit.Failed, i18n.T(c, "pairing_relay_invalid"))
			}
		})
		return
	}
	ctx, cancel := context.WithCancel(r.ctx)
	state.shareCancel = cancel
	r.App.Settings.Share = settings.ShareState{Kind: "checking"}
	if previous := state.shareChange; previous != nil && state.shareChangeKey == draftKey {
		r.shareOnNetwork(ctx, previous.selected, previous, delay, current)
		return
	}
	r.useConnectionSelection(state.picker, input, retry, func(selected protocol.NetworkSelection, err error) {
		if !current() || ctx.Err() != nil || errors.Is(err, context.Canceled) {
			cancel()
			return
		}
		if err != nil {
			cancel()
			state.shareCancel = nil
			r.App.Settings.Share = settings.ShareState{Kind: "failed"}
			r.enqueue(func(c *ui.Context) {
				if current() {
					r.reportConnection(c, err, connectionRelayFailureKey(err))
				}
			})
			return
		}
		if !sameConnectionNetwork(&input, &selected) {
			// Shorthand already waited for the picker's typing debounce.
			delay = 0
		}
		r.shareOnNetwork(ctx, selected, nil, delay, current)
	})
}

func (r *Runtime) shareOnNetwork(ctx context.Context, selected protocol.NetworkSelection, previous *connectionNetworkChange, delay time.Duration, current func() bool) {
	state := r.connections
	cancel, draftKey := state.shareCancel, state.shareSelection
	origin, endpoint := r.Preferences.Data.Pairing, state.endpoint
	r.async(func() {
		defer cancel()
		if delay > 0 {
			timer := time.NewTimer(delay)
			defer timer.Stop()
			select {
			case <-ctx.Done():
				return
			case <-timer.C:
			}
		}
		var change *connectionNetworkChange
		var err error
		if previous != nil || endpoint.NetworkSelection() != nil {
			change, err = r.prepareConnectionShare(ctx, selected, previous)
		}
		if ctx.Err() != nil {
			return
		}
		if err != nil {
			r.enqueue(func(c *ui.Context) {
				if !current() {
					return
				}
				state.shareCancel = nil
				r.App.Settings.Share = settings.ShareState{Kind: "failed"}
				state.shareChange, state.shareChangeKey = nil, ""
				if faultCode(err) == protocol.ErrorCodeOutcomeUnknown {
					state.shareChange, state.shareChangeKey = change, draftKey
				}
				if faultCode(err) == protocol.ErrorCodeUnavailable {
					kit.Toast(c, kit.Uncertain, i18n.T(c, "host_network_unknown"))
					return
				}
				r.reportConnection(c, err, connectionRelayFailureKey(err))
			})
			return
		}
		r.enqueue(func(c *ui.Context) {
			if current() {
				state.shareChange, state.shareChangeKey = nil, ""
				r.App.Settings.Share = settings.ShareState{Kind: "preparing"}
				if selected.Kind == "custom" {
					kit.Toast(c, kit.Done, i18n.T(c, "pairing_relay_ready"))
				}
			}
		})
		session, err := endpoint.Share(ctx, origin)
		if err != nil {
			r.enqueue(func(c *ui.Context) {
				if current() {
					state.shareCancel = nil
					r.App.Settings.Share = settings.ShareState{Kind: "failed"}
					kit.Toast(c, kit.Failed, i18n.T(c, "pairing_invalid_endpoint"))
				}
			})
			return
		}
		defer session.Close()
		for {
			value, err := session.Next(ctx)
			if ctx.Err() != nil {
				return
			}
			if err != nil {
				if !errors.Is(err, io.EOF) {
					r.enqueue(func(c *ui.Context) {
						if current() {
							state.shareCancel = nil
							r.App.Settings.Share = settings.ShareState{Kind: "failed"}
							r.reportConnection(c, err, "pairing_failed")
						}
					})
				}
				return
			}
			r.enqueue(func(c *ui.Context) {
				if !current() {
					return
				}
				model := r.App.Settings.Model
				previous := model.Share.Kind
				model.Share = settings.ShareState{Kind: value.Kind, Code: value.Code}
				switch value.Kind {
				case "retrying":
					if previous != "retrying" {
						kit.Toast(c, kit.Failed, i18n.T(c, "pairing_retrying"))
					}
				case "paired":
					state.shareCancel = nil
					model.CloseShare()
					kit.Toast(c, kit.Done, i18n.T(c, "pairing_success"))
				case "closed":
					state.shareCancel = nil
				}
			})
		}
	})
}

func (r *Runtime) prepareConnectionShare(ctx context.Context, selected protocol.NetworkSelection, previous *connectionNetworkChange) (*connectionNetworkChange, error) {
	if previous != nil {
		_, err := r.applyConnectionNetwork(ctx, previous, true)
		return previous, err
	}
	current, err := readConnectionNetwork(ctx, r.controller)
	if err != nil {
		return nil, err
	}
	if sameConnectionNetwork(current.Selection, &selected) && sameConnectionNetwork(current.Effective, &selected) {
		return nil, nil
	}
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	command := protocol.Command{Kind: "save_network_settings", Data: protocol.CommandSaveNetworkSettingsPayload{ExpectedRevision: current.Revision, Selection: selected}}
	change := &connectionNetworkChange{controller: r.controller, previous: current, selected: selected, request: r.controller.Prepare(command)}
	_, err = r.applyConnectionNetwork(ctx, change, false)
	return change, err
}
