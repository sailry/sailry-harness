package live

import (
	"context"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// recoverSSH asks the Node what became of an installation or update whose
// result was not seen, through the one resolver every such request uses.
func (r *Runtime) recoverSSH(operation *sshInstallation) {
	if operation.running {
		return
	}
	if operation.networkFix != nil {
		r.recoverSSHNetwork(operation)
		return
	}
	if operation.partial || operation.request == nil {
		if operation.expected != nil {
			r.releaseHostUpdate(operation)
			return
		}
		r.enqueue(func(c *ui.Context) {
			r.failSSH(c, operation, &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "Host installation must be checked on the target"})
		})
		return
	}
	request := *operation.request
	operation.running = true
	r.async(func() {
		resolution, err := r.controller.Resolve(r.ctx, request)
		r.enqueue(func(c *ui.Context) {
			if operation.request == nil || operation.request.ID != request.ID {
				return
			}
			operation.running = false
			if !operation.active {
				return
			}
			if err != nil {
				r.failSSH(c, operation, err)
				return
			}
			switch resolution.State {
			case client.RequestApplied:
				r.completeSSH(c, operation, resolution.Output, nil)
			case client.RequestFailed:
				// The Node recorded the failure: nothing is left to check.
				operation.request, operation.uncertain = nil, false
				r.endSSH(c, operation, resolution.Fault, false)
			case client.RequestAbsent:
				operation.running = true
				r.async(func() {
					output, executeErr := r.controller.Execute(r.ctx, request)
					r.enqueue(func(c *ui.Context) {
						operation.running = false
						if operation.active {
							r.completeSSH(c, operation, output, executeErr)
						}
					})
				})
			case client.RequestPending:
				r.failSSH(c, operation, &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "Host installation is still running"})
			case client.RequestUnknown:
				// The Node can never tell. An update is answered by the Host
				// itself; an installation is the user's to check on the target.
				if operation.expected != nil {
					r.toastSSH(c, resolution.Fault, true, true)
					r.releaseHostUpdate(operation)
					return
				}
				operation.partial = true
				r.endSSH(c, operation, resolution.Fault, true)
			}
		})
	})
}

// releaseHostUpdate ends an update whose result the Node cannot confirm. The
// request is given up, so the next Apply is a new one, and the Host is asked
// which release it runs: the same release shows it as current, an older one
// offers the update again, and an unreachable Host offers Check again. The
// saved connection stays for the next Apply.
func (r *Runtime) releaseHostUpdate(operation *sshInstallation) {
	operation.partial, operation.uncertain, operation.active = false, false, false
	operation.request, operation.retry = nil, protocol.Command{}
	operation.state.update.form = false
	r.checkHostUpdate(operation.state)
}

func (r *Runtime) recoverSSHNetwork(operation *sshInstallation) {
	if operation.running {
		return
	}
	fix := *operation.networkFix
	operation.running = true
	r.async(func() {
		err := r.confirmConnectionNetwork(r.ctx, fix.Node, fix.Selection)
		r.enqueue(func(c *ui.Context) {
			operation.running = false
			if !operation.active {
				return
			}
			if err != nil {
				r.failSSH(c, operation, err)
				return
			}
			r.completeSSH(c, operation, protocol.Output{Kind: "ssh_outcome", Data: protocol.SshOutcome{Kind: "host_installed", Data: protocol.SshOutcomeHostInstalledPayload{Node: fix.Node}}}, nil)
		})
	})
}

func (r *Runtime) confirmConnectionNetwork(ctx context.Context, node protocol.NodeID, selection protocol.NetworkSelection) error {
	if r.network == nil || r.network.endpoint == nil {
		return unknownConnectionNetwork()
	}
	endpoint := r.network.endpoint
	peers, err := endpoint.Peers(ctx)
	if err != nil {
		return unknownConnectionNetwork()
	}
	for _, previous := range peers {
		if previous.ID != node {
			continue
		}
		controller := client.New(endpoint.Remote(previous))
		defer controller.Close()
		actual, err := readConnectionNetwork(ctx, controller)
		if err != nil {
			candidate, addressErr := link.CandidateAddress(previous, selection)
			if addressErr != nil {
				return unknownConnectionNetwork()
			}
			if err := endpoint.UpdatePeer(ctx, candidate); err != nil {
				return unknownConnectionNetwork()
			}
			actual, err = readConnectionNetwork(ctx, controller)
		}
		if err != nil || actual.Revision == 0 || !sameConnectionNetwork(actual.Selection, &selection) || !sameConnectionNetwork(actual.Effective, &selection) {
			return unknownConnectionNetwork()
		}
		address, err := connectionNetworkAddress(actual, node)
		if err != nil {
			return unknownConnectionNetwork()
		}
		return endpoint.UpdatePeer(ctx, address)
	}
	return unknownConnectionNetwork()
}
