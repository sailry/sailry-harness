package live

import (
	"errors"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/host"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
	"golang.org/x/mod/semver"
)

type hostUpdate struct {
	form  bool
	draft settings.ConnectionDraft
}

func hostUpdateAvailable(current string) bool {
	current = "v" + strings.TrimPrefix(current, "v")
	target := "v" + strings.TrimPrefix(link.ProductVersion, "v")
	return semver.IsValid(current) && semver.IsValid(target) && semver.Compare(target, current) > 0
}

func (r *Runtime) advanceHostUpdate(id string, expected host.Update) {
	state := r.hostState(id)
	if state == nil || state.view.Update == nil || *state.view.Update != expected {
		return
	}
	action, _, ok := expected.Primary()
	if !ok || state.install != nil && (state.install.active || state.install.running) {
		return
	}
	if state.install != nil && (state.install.uncertain || state.install.partial || state.install.networkFix != nil) {
		state.install.active = true
		state.update.form = true
		state.view.Update.Error = ""
		r.recoverSSH(state.install)
		return
	}
	switch action {
	case host.Check:
		r.checkHostUpdate(state)
	case host.Download:
		state.view.Update.Phase = host.Staged
		state.view.Update.Note = "host_update_local_artifacts"
		state.view.Update.Progress = -1
	case host.Apply:
		if state.install != nil && state.install.profile != nil && state.install.retry.Kind != "" {
			state.install.active = true
			state.update.form = true
			state.view.Update.Error = ""
			r.executeSSH(state.install, state.install.retry)
			return
		}
		state.update.form = true
		if state.update.draft.Port == "" {
			state.update.draft = settings.ConnectionDraft{Method: settings.ByPassword, Port: "22", Name: state.view.Name}
		}
	}
}

func (r *Runtime) checkHostUpdate(state *liveHost) {
	view := state.view.Update
	view.Phase, view.Error, view.Note, view.Progress = host.Checking, "", "", -1
	if r.network == nil || r.network.endpoint == nil {
		r.enqueue(func(c *ui.Context) {
			r.failHostCheck(c, state, errors.New("Host network is unavailable"))
		})
		return
	}
	network := r.network
	var peer *networkPeer
	for _, candidate := range network.snapshot() {
		if candidate.address.ID.String() == state.view.ID {
			copy := candidate
			peer = &copy
			break
		}
	}
	if peer == nil {
		r.enqueue(func(c *ui.Context) {
			r.failHostCheck(c, state, errors.New("Host is no longer paired"))
		})
		return
	}
	r.async(func() {
		info, err := network.endpoint.Inspect(r.ctx, peer.address)
		r.enqueue(func(c *ui.Context) {
			if r.hostState(state.view.ID) != state || state.view.Update.Phase != host.Checking {
				return
			}
			if err != nil {
				r.failHostCheck(c, state, err)
				return
			}
			current, target := "", strings.TrimPrefix(link.ProductVersion, "v")
			if info.SoftwareVersion != nil {
				current = strings.TrimPrefix(*info.SoftwareVersion, "v")
			}
			if !info.Execution || !semver.IsValid("v"+current) || !semver.IsValid("v"+target) {
				r.failHostCheck(c, state, errors.New("Host version could not be verified"))
				return
			}
			view.Current, view.Target, view.Phase = current, target, host.Current
			view.Note = "host_update_local_artifacts"
			if hostUpdateAvailable(current) {
				view.Phase = host.Available
			}
			state.view.Runtime.Version = current
		})
	})
}

func (r *Runtime) failHostCheck(c *ui.Context, state *liveHost, err error) {
	state.view.Update.Phase, state.view.Update.Error, state.view.Update.Retry = host.UpdateFailed, err.Error(), host.Check
	r.reportSSH(c, err, true)
}

func (r *Runtime) hostUpdateFrame(c *ui.Context, state *liveHost) {
	update := state.update
	if !update.form {
		return
	}
	title := i18n.T(c, "host_update")
	pending := state.install != nil && (state.install.active || state.install.running)
	recovery := state.install != nil && (state.install.uncertain || state.install.partial || state.install.networkFix != nil)
	if state.install != nil && state.install.trusting && r.App.Settings.HostTrust == state.install.prompt && state.install.prompt != nil {
		trust := r.App.Settings.HostTrust
		trustTitle := i18n.T(c, "ssh_trust_title")
		if trust.Changed {
			trustTitle = i18n.T(c, "ssh_key_changed")
		}
		open := true
		if kit.AlertDialog(c.Key("host-update-key-"+state.view.ID), &open, trustTitle, trust.Host+"\n"+trust.Algorithm+"\n"+trust.Fingerprint, i18n.T(c, "settings_cancel"), i18n.T(c, "ssh_trust")) == 1 {
			r.App.Settings.HostTrust = nil
			trust.Confirm()
		}
		if !open {
			r.App.Settings.HostTrust = nil
		}
		return
	}
	width, _ := c.Size()
	open := true
	kit.Dialog(c.Key("host-update-credentials-"+state.view.ID), &open, func() {
		ui.Text(c, title).FontSize(16).FontWeight(600)
		ui.Text(c, i18n.T(c, "host_update_restart")).FontSize(14)
		if pending {
			stage := sshProgressKey(state.install.stage)
			kit.Shimmer(c, i18n.T(c, stage), 14, theme.Of(c).MutedForeground)
			ui.Progress(c.Key("progress"), -1).FillWidth().Height(4).Label(i18n.T(c, stage))
		} else if !recovery {
			var saved *protocol.SshProfile
			if state.install != nil {
				saved = state.install.profile
			}
			r.sshCredentialFields(c, &update.draft, saved)
		}
		ui.Row(c.Key("footer")).Gap(8).Justify(ui.End).Children(func() {
			kit.Secondary(c.Key("cancel"), i18n.T(c, "settings_cancel")).OnClick(func() {
				update.form = false
				if state.install != nil && state.install.active {
					r.cancelSSH(state.install)
					state.view.Update.Phase = host.Staged
				}
			})
			label := i18n.T(c, "live_update_apply")
			if recovery {
				label = i18n.T(c, "live_update_retry")
			}
			kit.Primary(c.Key("apply"), label, false).Disabled(pending).Children(func() { ui.Text(c, label).FontSize(14) }).OnClick(func() {
				if recovery {
					state.install.active = true
					r.recoverSSH(state.install)
					return
				}
				if state.install != nil && state.install.draft == update.draft && state.install.retry.Kind != "" {
					state.install.active = true
					r.executeSSH(state.install, state.install.retry)
					return
				}
				// The connection this Host was updated through is saved
				// again at its revision, keeping a credential left blank.
				var saved *protocol.SshProfile
				if state.install != nil {
					saved = state.install.profile
				}
				command, err := sshSaveCommand(update.draft, saved)
				if err != nil {
					r.reportSSH(c, err, true)
					return
				}
				node := state.node
				operation := &sshInstallation{state: state, draft: update.draft, profile: saved, expected: &node, active: true, stage: "connecting", completed: func(ok bool) {
					if ok {
						update.form = false
						update.draft.Password, update.draft.Key, update.draft.Passphrase = "", "", ""
					}
				}}
				state.install = operation
				state.view.Update.Error = ""
				r.executeSSH(operation, command)
			})
		})
	}).Width(min(520, width-48)).Gap(16).Label(title)
	if !open {
		update.form = false
		if state.install != nil && state.install.active {
			r.cancelSSH(state.install)
			state.view.Update.Phase = host.Staged
		}
	}
}
