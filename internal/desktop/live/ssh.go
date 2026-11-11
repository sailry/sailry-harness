package live

import (
	"errors"
	"net"
	"strconv"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/host"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type sshInstallation struct {
	state      *liveHost
	draft      settings.ConnectionDraft
	profile    *protocol.SshProfile
	expected   *protocol.NodeID
	network    *protocol.NetworkSelection
	request    *protocol.Request
	retry      protocol.Command
	completed  func(bool)
	active     bool
	running    bool
	uncertain  bool
	partial    bool
	trusting   bool
	prompt     *settings.HostTrust
	stage      string
	polling    bool
	nextPoll   time.Time
	networkFix *protocol.SshOutcomeHostNetworkUnconfirmedPayload
}

func (r *Runtime) addSSHHost(draft settings.ConnectionDraft, completed func(bool)) {
	root := r.localRuntime()
	local := root.hostState(root.controller.Target().String())
	if local == nil {
		root.initHosts()
		local = root.hostState(root.controller.Target().String())
	}
	if completed == nil {
		completed = func(bool) {}
	}
	if operation := local.install; operation != nil && (operation.active || operation.running) {
		// A step cancelled here may still run on the Node; the next waits for it.
		root.enqueue(func(c *ui.Context) {
			kit.Toast(c, kit.Failed, i18n.T(c, "ssh_busy"))
			completed(false)
		})
		return
	}
	selection, err := root.connectionSelection(draft)
	if err != nil {
		root.enqueue(func(c *ui.Context) {
			root.reportConnection(c, err, "pairing_relay_invalid")
			completed(false)
		})
		return
	}
	// A changed draft updates the connection this dialog already saved
	// instead of saving another.
	var saved *protocol.SshProfile
	if operation := local.install; operation != nil {
		saved = operation.profile
		sameTarget := sameSSHHostDraft(operation.draft, draft) && sameConnectionNetwork(operation.network, &selection)
		if sshRecoverable(operation) {
			if !sameTarget {
				root.enqueue(func(c *ui.Context) {
					root.reportSSH(c, &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "The original Host installation must be checked before changing its target"}, false)
					completed(false)
				})
				return
			}
			operation.completed, operation.active = completed, true
			root.App.Settings.ConnectionPending = true
			root.recoverSSH(operation)
			return
		}
		if sameTarget && operation.retry.Kind != "" {
			operation.completed, operation.active = completed, true
			root.App.Settings.ConnectionPending = true
			root.executeSSH(operation, operation.retry)
			return
		}
	}
	// WSL installs in one step on this computer; SSH saves its connection first.
	command := protocol.Command{Kind: "install_wsl_host", Data: protocol.CommandInstallWslHostPayload{Distribution: draft.Distribution, Network: &selection}}
	if draft.Method == settings.ByWSL {
		saved = nil
	} else {
		command, err = sshSaveCommand(draft, saved)
	}
	if err != nil {
		root.enqueue(func(c *ui.Context) {
			root.reportSSH(c, err, false)
			completed(false)
		})
		return
	}
	operation := &sshInstallation{state: local, draft: draft, profile: saved, network: &selection, completed: completed, active: true, stage: "connecting"}
	local.install = operation
	root.App.Settings.ConnectionPending = true
	root.executeSSH(operation, command)
}

// checkSSHHost asks what became of the installation whose outcome is
// uncertain: its receipt on the local Node, or the installed Host's network.
// It never sends the installation again.
func (r *Runtime) checkSSHHost(completed func(bool)) {
	root := r.localRuntime()
	local := root.hostState(root.controller.Target().String())
	if completed == nil {
		completed = func(bool) {}
	}
	if local == nil || local.install == nil || !sshRecoverable(local.install) {
		completed(false)
		return
	}
	operation := local.install
	operation.completed, operation.active, operation.stage = completed, true, "connecting"
	root.App.Settings.ConnectionPending = true
	root.recoverSSH(operation)
}

// sshRecoverable is an idle installation whose outcome has to be checked
// before anything else is sent for it.
func sshRecoverable(operation *sshInstallation) bool {
	return !operation.active && !operation.running && (operation.uncertain || operation.partial || operation.networkFix != nil)
}

// sshUncertain is whether a failure leaves its step's outcome unknown. A
// connection test changes nothing, so its unreachable target, refused login
// or missing agent is a plain failure.
func sshUncertain(request *protocol.Request, err error) bool {
	switch faultCode(err) {
	case protocol.ErrorCodeOutcomeUnknown:
		return true
	case protocol.ErrorCodeUnavailable:
		return request == nil || request.Command.Kind != "check_ssh"
	}
	return false
}

// sshSaveCommand saves the draft's connection: a new one, or the saved one at
// its revision with its trusted host key. A secret left blank under the same
// authentication keeps the saved credential.
func sshSaveCommand(draft settings.ConnectionDraft, saved *protocol.SshProfile) (protocol.Command, error) {
	profile, credential, err := sshDraft(draft)
	if err != nil {
		return protocol.Command{}, err
	}
	payload := protocol.CommandSaveSshPayload{Profile: profile, Credential: &credential}
	if saved != nil {
		payload.Profile.ID, payload.Profile.HostKey, payload.ExpectedRevision = saved.ID, saved.HostKey, saved.Revision
		blank := draft.Method == settings.ByAgent || draft.Method == settings.ByPassword && draft.Password == "" || draft.Method != settings.ByPassword && draft.Key == ""
		if saved.Authentication == profile.Authentication && blank {
			payload.Credential = nil
		}
	}
	return protocol.Command{Kind: "save_ssh", Data: payload}, nil
}

func sameSSHHostDraft(first, second settings.ConnectionDraft) bool {
	first.Relay, second.Relay = 0, 0
	first.Address, second.Address = "", ""
	first.Code, second.Code = "", ""
	return first == second
}

func sshDraft(draft settings.ConnectionDraft) (protocol.SshProfile, protocol.SshCredential, error) {
	port := uint64(22)
	var err error
	if strings.TrimSpace(draft.Port) != "" {
		port, err = strconv.ParseUint(strings.TrimSpace(draft.Port), 10, 16)
	}
	if err != nil || port == 0 {
		return protocol.SshProfile{}, protocol.SshCredential{}, errors.New("SSH port must be between 1 and 65535")
	}
	profile := protocol.SshProfile{ID: protocol.NewSshID(), Name: strings.TrimSpace(draft.Name), Host: strings.TrimSpace(draft.Host), Port: uint16(port), Username: strings.TrimSpace(draft.Username)}
	switch draft.Scope {
	case settings.ScopeGlobal:
		profile.Sharing = &protocol.ConnectionSharing{Kind: "global"}
	case settings.ScopeProjects:
		var projects []protocol.ProjectID
		for _, id := range strings.Split(draft.Projects, ",") {
			project, err := protocol.ParseProjectID(id)
			if err != nil {
				return protocol.SshProfile{}, protocol.SshCredential{}, errors.New("SSH sharing needs a project")
			}
			projects = append(projects, project)
		}
		profile.Sharing = &protocol.ConnectionSharing{Kind: "projects", Data: projects}
	}
	var passphrase *protocol.Secret
	if draft.Passphrase != "" {
		value := protocol.NewSecret(draft.Passphrase)
		passphrase = &value
	}
	credential := protocol.SshCredential{}
	switch draft.Method {
	case settings.ByPassword:
		profile.Authentication = protocol.SshAuthenticationPassword
		credential.Kind, credential.Data = "password", protocol.SshCredentialPasswordPayload{Password: protocol.NewSecret(draft.Password)}
	case settings.ByPrivateKey:
		profile.Authentication = protocol.SshAuthenticationPrivateKey
		credential.Kind, credential.Data = "private_key", protocol.SshCredentialPrivateKeyPayload{Key: protocol.NewSecret(draft.Key), Passphrase: passphrase}
	case settings.ByKeyFile:
		profile.Authentication = protocol.SshAuthenticationKeyPath
		credential.Kind, credential.Data = "key_path", protocol.SshCredentialKeyPathPayload{Path: protocol.NewSecret(draft.Key), Passphrase: passphrase}
	case settings.ByAgent:
		profile.Authentication = protocol.SshAuthenticationAgent
		credential.Kind = "agent"
	default:
		return protocol.SshProfile{}, protocol.SshCredential{}, errors.New("SSH authentication is required")
	}
	return profile, credential, nil
}

func (r *Runtime) executeSSH(operation *sshInstallation, command protocol.Command) {
	if operation.running || !operation.active {
		return
	}
	request := r.controller.Prepare(command)
	operation.request = &request
	operation.retry = command
	operation.running, operation.uncertain = true, false
	operation.nextPoll = time.Time{}
	r.async(func() {
		output, err := r.controller.Execute(r.ctx, request)
		r.enqueue(func(c *ui.Context) {
			if operation.request == nil || operation.request.ID != request.ID {
				return
			}
			operation.running = false
			if !operation.active {
				operation.uncertain = true
				// The Add host dialog that cancelled it is gone, and
				// with it what there was to check.
				if operation.expected == nil && operation.state.install == operation {
					operation.state.install = nil
				}
				return
			}
			r.completeSSH(c, operation, output, err)
		})
	})
}

func (r *Runtime) completeSSH(c *ui.Context, operation *sshInstallation, output protocol.Output, err error) {
	if err != nil {
		uncertain := sshUncertain(operation.request, err)
		if !uncertain {
			// A saved connection that changed or went away is not updated again.
			if code := faultCode(err); operation.request != nil && operation.request.Command.Kind == "save_ssh" &&
				(code == protocol.ErrorCodeRevisionConflict || code == protocol.ErrorCodeConflict || code == protocol.ErrorCodeNotFound) {
				operation.profile, operation.retry = nil, protocol.Command{}
			}
			operation.request = nil
		}
		r.endSSH(c, operation, err, uncertain)
		return
	}
	trusted := operation.request != nil && operation.request.Command.Kind == "trust_ssh"
	operation.request, operation.uncertain = nil, false
	switch output.Kind {
	case "host_installation":
		// A WSL installation ends as an SSH one does.
		outcome, ok := detail[protocol.HostInstallationOutcome](output.Data)
		if !ok {
			break
		}
		switch outcome.Kind {
		case "installed":
			if installed, ok := detail[protocol.HostInstallationOutcomeInstalledPayload](outcome.Data); ok {
				r.completeSSH(c, operation, protocol.Output{Kind: "ssh_outcome", Data: protocol.SshOutcome{Kind: "host_installed", Data: protocol.SshOutcomeHostInstalledPayload{Node: installed.Node}}}, nil)
				return
			}
		case "network_unconfirmed":
			if network, ok := detail[protocol.HostInstallationOutcomeNetworkUnconfirmedPayload](outcome.Data); ok {
				r.completeSSH(c, operation, protocol.Output{Kind: "ssh_outcome", Data: protocol.SshOutcome{Kind: "host_network_unconfirmed", Data: protocol.SshOutcomeHostNetworkUnconfirmedPayload{Node: network.Node, Selection: network.Selection}}}, nil)
				return
			}
		}
	case "ssh_profile":
		profile, ok := detail[protocol.SshProfile](output.Data)
		if !ok {
			break
		}
		operation.profile = &profile
		if trusted {
			r.deploySSH(operation)
		} else {
			r.executeSSH(operation, protocol.Command{Kind: "check_ssh", Data: protocol.CommandCheckSshPayload{Profile: profile.ID, ExpectedRevision: profile.Revision}})
		}
		return
	case "ssh_outcome":
		outcome, ok := detail[protocol.SshOutcome](output.Data)
		if !ok {
			break
		}
		switch outcome.Kind {
		case "connected":
			r.deploySSH(operation)
			return
		case "host_key_required":
			required, ok := detail[protocol.SshOutcomeHostKeyRequiredPayload](outcome.Data)
			if ok && operation.profile != nil {
				r.trustSSH(operation, required)
				return
			}
		case "host_installed":
			installed, ok := detail[protocol.SshOutcomeHostInstalledPayload](outcome.Data)
			if !ok {
				break
			}
			if operation.expected != nil && installed.Node != *operation.expected {
				r.failSSH(c, operation, &protocol.Fault{Code: protocol.ErrorCodeWrongTarget, Message: "Updated Host identity does not match"})
				return
			}
			operation.active, operation.retry, operation.partial, operation.networkFix = false, protocol.Command{}, false, nil
			key := "host_added"
			if operation.expected != nil {
				key = "host_updated"
				operation.state.view.Update = &host.Update{Phase: host.Current, Current: operation.state.view.Update.Target, Progress: -1}
				operation.state.view.Runtime.Version = operation.state.view.Update.Current
			} else {
				r.App.Settings.ConnectionPending = false
			}
			kit.Toast(c, kit.Done, i18n.T(c, key))
			if operation.completed != nil {
				operation.completed(true)
			}
			operation.draft.Password, operation.draft.Key, operation.draft.Passphrase = "", "", ""
			if r.network != nil {
				network := r.network
				r.async(network.discover)
			}
			r.project()
			return
		case "host_network_unconfirmed":
			network, ok := detail[protocol.SshOutcomeHostNetworkUnconfirmedPayload](outcome.Data)
			if ok {
				operation.networkFix = &network
				r.failSSH(c, operation, &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "Host network setup could not be confirmed"})
				return
			}
		}
	}
	r.failSSH(c, operation, errors.New("Host installation response is invalid"))
}

func (r *Runtime) deploySSH(operation *sshInstallation) {
	if operation.profile == nil {
		return
	}
	operation.stage = "connecting"
	profile := operation.profile
	command := protocol.Command{Kind: "install_host", Data: protocol.CommandInstallHostPayload{Profile: profile.ID, ExpectedRevision: profile.Revision, Network: operation.network}}
	if operation.expected != nil {
		command = protocol.Command{Kind: "update_ssh_host", Data: protocol.CommandUpdateSshHostPayload{Profile: profile.ID, ExpectedRevision: profile.Revision, Node: *operation.expected}}
		operation.state.view.Update.Phase, operation.state.view.Update.Error = host.Applying, ""
	}
	r.executeSSH(operation, command)
}

func (r *Runtime) trustSSH(operation *sshInstallation, required protocol.SshOutcomeHostKeyRequiredPayload) {
	profile := *operation.profile
	operation.trusting = true
	operation.prompt = &settings.HostTrust{
		Host: net.JoinHostPort(profile.Host, strconv.Itoa(int(profile.Port))), Algorithm: required.Key.Algorithm,
		Fingerprint: required.Key.Fingerprint, Changed: required.Changed,
		Confirm: func() {
			if !operation.active || !operation.trusting {
				return
			}
			operation.trusting = false
			r.executeSSH(operation, protocol.Command{Kind: "trust_ssh", Data: protocol.CommandTrustSshPayload{Profile: profile.ID, ExpectedRevision: profile.Revision, Key: required.Key}})
		},
	}
	r.App.Settings.HostTrust = operation.prompt
}

func (r *Runtime) failSSH(c *ui.Context, operation *sshInstallation, err error) {
	r.endSSH(c, operation, err, sshUncertain(operation.request, err))
}

// endSSH stops the operation on a failure, which leaves its outcome to be
// checked when uncertain.
func (r *Runtime) endSSH(c *ui.Context, operation *sshInstallation, err error, uncertain bool) {
	if uncertain {
		operation.uncertain = true
	}
	operation.active, operation.trusting = false, false
	if operation.expected != nil {
		update := operation.state.view.Update
		update.Phase, update.Error, update.Retry, update.Progress = host.Staged, err.Error(), host.Apply, -1
	} else {
		r.App.Settings.ConnectionPending = false
		// An outcome nothing can confirm is the user's to check on the
		// host: the toast says so and the dialog takes a new draft.
		if operation.partial && operation.state.install == operation {
			operation.state.install = nil
		}
	}
	r.toastSSH(c, err, operation.expected != nil, uncertain)
	if operation.completed != nil {
		operation.completed(false)
	}
}

func (r *Runtime) reportSSH(c *ui.Context, err error, updating bool) {
	r.toastSSH(c, err, updating, sshUncertain(nil, err))
}

func (r *Runtime) toastSSH(c *ui.Context, err error, updating, uncertain bool) {
	key, tone := "host_install_failed", kit.Failed
	if updating {
		key = "live_update_failed"
	}
	switch code := faultCode(err); {
	case code == protocol.ErrorCodeConflict || code == protocol.ErrorCodeRevisionConflict:
		key, tone = "host_network_conflict", kit.Conflict
	case uncertain:
		key, tone = "host_install_unknown", kit.Uncertain
	}
	kit.Toast(c, tone, i18n.T(c, key))
}

func (r *Runtime) sshProgress() string {
	state := r.hostState(r.controller.Target().String())
	if state == nil || state.install == nil || !state.install.active {
		return ""
	}
	return sshProgressKey(state.install.stage)
}

func sshProgressKey(stage string) string {
	switch stage {
	case "detecting":
		return "host_install_detecting"
	case "installing":
		return "host_install_installing"
	case "pairing":
		return "host_install_pairing"
	}
	return "connection_connecting"
}

func (r *Runtime) sshFrame(c *ui.Context, operation *sshInstallation) {
	// The Add host dialog owns its installation: closed, it leaves nothing
	// to check or retry, and the next one starts anew.
	if operation.expected == nil && !operation.active && !operation.running && !r.App.Settings.Adding() {
		if operation.state.install == operation {
			operation.state.install = nil
		}
		return
	}
	if !operation.active {
		return
	}
	if operation.trusting {
		if r.App.Settings.HostTrust != operation.prompt {
			operation.trusting, operation.active = false, false
			if operation.expected == nil {
				r.App.Settings.ConnectionPending = false
			} else {
				operation.state.view.Update.Phase = host.Staged
			}
			if operation.completed != nil {
				operation.completed(false)
			}
		}
		return
	}
	if operation.expected == nil && !r.App.Settings.Adding() {
		r.cancelSSH(operation)
		return
	}
	if operation.request == nil || !operation.running {
		return
	}
	switch operation.request.Command.Kind {
	case "install_host", "update_ssh_host", "install_wsl_host":
	default:
		return
	}
	if !operation.polling && !c.Now().Before(operation.nextPoll) {
		r.pollSSH(operation)
	}
	c.After(300 * time.Millisecond)
}

func (r *Runtime) pollSSH(operation *sshInstallation) {
	request := operation.request.ID
	operation.polling = true
	read := protocol.Command{Kind: "read_host_install", Data: protocol.CommandReadHostInstallPayload{Request: request}}
	if operation.request.Command.Kind == "install_wsl_host" {
		read = protocol.Command{Kind: "read_wsl_host_install", Data: protocol.CommandReadWslHostInstallPayload{Request: request}}
	}
	r.async(func() {
		output, err := r.controller.Do(r.ctx, read)
		r.enqueue(func(c *ui.Context) {
			operation.polling, operation.nextPoll = false, c.Now().Add(300*time.Millisecond)
			if !operation.active || operation.request == nil || operation.request.ID != request || err != nil {
				return
			}
			if progress, ok := detail[protocol.SshInstallProgress](output.Data); output.Kind == "host_install_progress" && ok {
				operation.stage = progress.Kind
				if operation.expected != nil {
					operation.state.view.Update.Phase = host.Applying
					if progress.Kind == "pairing" {
						operation.state.view.Update.Phase = host.RestartWait
					}
				}
			}
		})
	})
}

func (r *Runtime) cancelSSH(operation *sshInstallation) {
	operation.active, operation.trusting = false, false
	if r.App.Settings.HostTrust == operation.prompt {
		r.App.Settings.HostTrust = nil
	}
	if operation.expected == nil {
		r.App.Settings.ConnectionPending = false
	}
	if operation.request == nil {
		return
	}
	request := *operation.request
	switch request.Command.Kind {
	case "check_ssh", "install_host", "update_ssh_host":
		r.async(func() {
			_, _ = r.controller.Do(r.ctx, protocol.Command{Kind: "cancel_ssh", Data: protocol.CommandCancelSshPayload{Request: request.ID}})
		})
	case "install_wsl_host":
		r.async(func() {
			_, _ = r.controller.Do(r.ctx, protocol.Command{Kind: "cancel_wsl_host_install", Data: protocol.CommandCancelWslHostInstallPayload{Request: request.ID}})
		})
	}
}
