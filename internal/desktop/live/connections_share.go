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

func (r *Runtime) startConnectionShare(delay time.Duration) {
	state := r.connections
	if state == nil || !r.App.Settings.Sharing() {
		return
	}
	r.stopConnectionShare()
	selected, err := r.connectionSelection(r.App.Settings.ConnectionDraft())
	if err != nil || state.endpoint == nil {
		r.App.Settings.Share = settings.ShareState{Kind: "failed"}
		r.enqueue(func(c *ui.Context) { kit.Toast(c, kit.Failed, i18n.T(c, "pairing_relay_invalid")) })
		return
	}
	ctx, cancel := context.WithCancel(r.ctx)
	state.shareCancel = cancel
	generation := state.shareGeneration
	origin, endpoint := r.Preferences.Data.Pairing, state.endpoint
	previous := state.shareChange
	if previous != nil && !sameConnectionNetwork(&previous.selected, &selected) {
		previous = nil
	}
	r.App.Settings.Share = settings.ShareState{Kind: "checking"}
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
		if endpoint.NetworkSelection() != nil {
			change, err = r.prepareConnectionShare(ctx, selected, previous)
		}
		if ctx.Err() != nil {
			return
		}
		if err != nil {
			r.enqueue(func(c *ui.Context) {
				if state.shareGeneration != generation {
					return
				}
				state.shareCancel = nil
				r.App.Settings.Share = settings.ShareState{Kind: "failed"}
				state.shareChange = nil
				if faultCode(err) == protocol.ErrorCodeOutcomeUnknown {
					state.shareChange = change
				}
				key := "pairing_relay_failed"
				if faultCode(err) == protocol.ErrorCodeInvalidRequest {
					key = "pairing_relay_invalid"
				}
				if faultCode(err) == protocol.ErrorCodeUnavailable {
					kit.Toast(c, kit.Uncertain, i18n.T(c, "host_network_unknown"))
					return
				}
				r.reportConnection(c, err, key)
			})
			return
		}
		r.enqueue(func(c *ui.Context) {
			if state.shareGeneration == generation {
				state.shareChange = nil
				r.App.Settings.Share = settings.ShareState{Kind: "preparing"}
				if selected.Kind == "custom" {
					kit.Toast(c, kit.Done, i18n.T(c, "pairing_relay_ready"))
				}
			}
		})
		session, err := endpoint.Share(ctx, origin)
		if err != nil {
			r.enqueue(func(c *ui.Context) {
				if state.shareGeneration == generation {
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
						if state.shareGeneration == generation {
							state.shareCancel = nil
							r.App.Settings.Share = settings.ShareState{Kind: "failed"}
							r.reportConnection(c, err, "pairing_failed")
						}
					})
				}
				return
			}
			r.enqueue(func(c *ui.Context) {
				if state.shareGeneration != generation {
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
